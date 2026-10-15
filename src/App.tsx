import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import "./App.css";
import {
  MODE_LABELS,
  TYPE_LABELS,
  type AppStateView,
  type DataField,
  type FieldType,
  type GenMode,
  type InstanceType,
  type ModbusRegisterDef,
  type SimEvent,
  type SimInstance,
} from "./types";

const FIELD_TYPES = [
  "float",
  "int",
  "bool",
  "string",
  "int16",
  "uint16",
  "int32",
  "uint32",
  "float32",
  "hex",
] as const;

const MODBUS_FNS = [
  "read_coils",
  "read_discrete_inputs",
  "read_holding_registers",
  "read_input_registers",
  "write_coil",
  "write_register",
  "write_coils",
  "write_registers",
] as const;

const TABLES = ["coils", "discrete_inputs", "holding_registers", "input_registers"] as const;
const BYTE_ORDERS = ["ABCD", "CDAB", "BADC", "DCBA"] as const;

/** 数字输入：允许清空后重输，避免 Number("") 变成 0 打断编辑 */
function parseNumberInput(raw: string, fallback: number): number {
  if (raw.trim() === "") return fallback;
  const n = Number(raw);
  return Number.isFinite(n) ? n : fallback;
}

function formatConstValue(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "object") {
    try {
      return JSON.stringify(value);
    } catch {
      return String(value);
    }
  }
  return String(value);
}

function parseConstValue(text: string, fieldType: FieldType): unknown {
  const raw = text.trim();
  if (raw === "") return null;
  if (fieldType === "bool") {
    return ["1", "true", "yes", "on"].includes(raw.toLowerCase());
  }
  if (fieldType === "string" || fieldType === "hex") return raw;
  if (raw === "true" || raw === "false") return raw === "true";
  const n = Number(raw);
  if (Number.isFinite(n)) {
    if (fieldType === "float" || fieldType === "float32") return n;
    return Math.round(n);
  }
  return raw;
}

function emptyState(): AppStateView {
  return {
    profile: { version: 1, name: "未命名方案", description: "", instances: [] },
    profile_path: null,
    running: {},
    stats: {},
  };
}

export default function App() {
  const [state, setState] = useState<AppStateView>(emptyState);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<SimInstance | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [logKind, setLogKind] = useState("all");
  const [toast, setToast] = useState<{ text: string; error?: boolean } | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [path, setPath] = useState("");

  const notify = useCallback((text: string, error = false) => {
    setToast({ text, error });
    window.setTimeout(() => setToast(null), 2600);
  }, []);

  const refresh = useCallback(async (next?: AppStateView) => {
    const s = next ?? (await api.getState());
    setState(s);
    if (s.profile_path) setPath(s.profile_path);
    return s;
  }, []);

  useEffect(() => {
    refresh()
      .then((s) => {
        if (s.profile.instances.length) {
          setSelectedId(s.profile.instances[0].id);
        }
      })
      .catch((e) => notify(String(e), true));
  }, [refresh, notify]);

  const loadDraft = useCallback((s: AppStateView, id: string | null) => {
    if (!id) {
      setDraft(null);
      return;
    }
    const inst = s.profile.instances.find((i) => i.id === id) ?? null;
    setDraft(inst ? structuredClone(inst) : null);
  }, []);

  // 仅在切换选中实例时加载编辑稿，避免轮询把未应用的修改冲掉
  useEffect(() => {
    loadDraft(state, selectedId);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 故意不依赖 state.profile
  }, [selectedId, loadDraft]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<SimEvent>("sim-event", (event) => {
      const e = event.payload;
      if (logKind !== "all" && e.kind !== logKind && e.level !== logKind) return;
      const ts = new Date(e.ts * 1000).toLocaleTimeString();
      const line = `${ts}  ${e.level.toUpperCase().padEnd(5)}  ${e.instance_name.padEnd(16)}  ${e.kind.padEnd(10)}  ${e.message}`;
      setLogs((prev) => [...prev.slice(-399), line]);
    }).then((fn) => {
      unlisten = fn;
    });
    // 轮询只刷新运行状态/统计，不覆盖 profile，避免打断编辑
    const timer = window.setInterval(() => {
      api
        .getState()
        .then((s) => {
          setState((prev) => ({
            ...prev,
            running: s.running,
            stats: s.stats,
          }));
        })
        .catch(() => undefined);
    }, 1000);
    return () => {
      unlisten?.();
      window.clearInterval(timer);
    };
  }, [logKind]);

  const selectedStats = selectedId ? state.stats[selectedId] : undefined;
  const selectedRunning = selectedId ? !!state.running[selectedId] : false;

  const totals = useMemo(() => {
    let sent = 0;
    let received = 0;
    let errors = 0;
    let running = 0;
    for (const inst of state.profile.instances) {
      const st = state.stats[inst.id];
      if (st) {
        sent += st.sent;
        received += st.received;
        errors += st.errors;
      }
      if (state.running[inst.id]) running += 1;
    }
    return { sent, received, errors, running };
  }, [state]);

  async function withState(
    fn: () => Promise<AppStateView>,
    ok?: string,
    opts?: { syncDraft?: boolean },
  ) {
    try {
      const s = await fn();
      await refresh(s);
      if (opts?.syncDraft !== false) {
        // 结构变更后同步编辑稿；纯启停可传 syncDraft:false 保留未应用修改
        loadDraft(s, selectedId);
      }
      if (ok) notify(ok);
      return s;
    } catch (e) {
      notify(String(e), true);
      return null;
    }
  }

  async function onAdd(type: InstanceType) {
    setMenuOpen(false);
    const s = await withState(() => api.addInstance(type), "已创建实例");
    if (s?.profile.instances.length) {
      setSelectedId(s.profile.instances[s.profile.instances.length - 1].id);
    }
  }

  async function onOpen() {
    const file = await open({
      multiple: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!file || Array.isArray(file)) return;
    const s = await withState(() => api.loadProfile(file), "已加载方案");
    if (s?.profile.instances[0]) setSelectedId(s.profile.instances[0].id);
  }

  async function flushDraft(): Promise<AppStateView | null> {
    if (!draft) return state;
    try {
      const result = await api.updateInstance(draft);
      const { warning, ...rest } = result as AppStateView & { warning?: string | null };
      const s: AppStateView = {
        profile: rest.profile,
        profile_path: rest.profile_path,
        running: rest.running,
        stats: rest.stats,
      };
      setState(s);
      if (s.profile_path) setPath(s.profile_path);
      loadDraft(s, selectedId);
      if (warning) notify(warning, true);
      return s;
    } catch (e) {
      notify(`应用编辑失败，未写入文件: ${e}`, true);
      return null;
    }
  }

  async function onSave() {
    // 先把界面编辑稿写入引擎，再落盘（避免“改了但文件没变”）
    const flushed = await flushDraft();
    if (draft && !flushed) return;

    let target = (path || state.profile_path || "").trim() || undefined;
    if (!target) {
      const file = await save({
        filters: [{ name: "JSON", extensions: ["json"] }],
        defaultPath: `${state.profile.name || "profile"}.json`,
      });
      if (!file) return;
      target = file;
      setPath(file);
    }
    try {
      const s = await api.saveProfile(target);
      setState(s);
      if (s.profile_path) setPath(s.profile_path);
      loadDraft(s, selectedId);
      notify(`已保存 ${target}`);
    } catch (e) {
      notify(`保存失败: ${e}`, true);
    }
  }

  async function onExport() {
    try {
      const text = await api.dumpProfile();
      await navigator.clipboard.writeText(text);
      notify("方案已复制到剪贴板");
    } catch (e) {
      notify(String(e), true);
    }
  }

  async function onImportFile(file: File) {
    const text = await file.text();
    const s = await withState(() => api.loadProfileJson(text), "方案已导入");
    if (s?.profile.instances[0]) setSelectedId(s.profile.instances[0].id);
  }

  async function applyDraft() {
    if (!draft) return;
    const s = await flushDraft();
    if (s) notify("配置已应用");
  }

  return (
    <div className="app">
      <header className="header">
        <div className="brand">
          <div className="brand-mark" />
          <div>
            <h1>DeviceSim</h1>
            <p>
              MQTT / Modbus TCP 设备仿真器 v1.0.0 · {state.profile.name} ·{" "}
              {state.profile.instances.length} 个实例
            </p>
          </div>
        </div>
        <div className="actions">
          <button
            className="btn primary"
            onClick={() => withState(() => api.startAll(), "已启动全部", { syncDraft: false })}
          >
            全部启动
          </button>
          <button
            className="btn danger"
            onClick={() => withState(() => api.stopAll(), "已全部停止", { syncDraft: false })}
          >
            全部停止
          </button>
          <div className="menu">
            <button className="btn" onClick={() => setMenuOpen((v) => !v)}>
              新建 ▾
            </button>
            {menuOpen && (
              <div className="menu-pop">
                <button onClick={() => onAdd("mqtt_client")}>MQTT 客户端</button>
                <button onClick={() => onAdd("modbus_tcp_client")}>Modbus TCP 客户端</button>
                <button onClick={() => onAdd("modbus_tcp_slave")}>Modbus TCP 从站</button>
              </div>
            )}
          </div>
          <button className="btn ghost" onClick={onOpen}>
            打开
          </button>
          <button className="btn ghost" onClick={onSave}>
            保存
          </button>
          <button className="btn ghost" onClick={onExport}>
            导出
          </button>
        </div>
      </header>

      <div className="layout">
        <aside className="sidebar">
          <p className="section-title">实例</p>
          <label className="field">
            方案文件路径
            <input value={path} onChange={(e) => setPath(e.target.value)} />
          </label>
          <div className="row" style={{ margin: "0.5rem 0" }}>
            <button className="btn" onClick={() => path && withState(() => api.loadProfile(path), "已加载")}>
              加载
            </button>
            <button className="btn" onClick={onSave}>
              保存到此路径
            </button>
          </div>
          <label className="field" style={{ marginBottom: "0.75rem" }}>
            导入 JSON
            <input
              type="file"
              accept=".json,application/json"
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) void onImportFile(f);
              }}
            />
          </label>

          {!state.profile.instances.length && (
            <p className="muted">还没有实例，点击右上角「新建」。</p>
          )}
          {state.profile.instances.map((inst) => {
            const running = !!state.running[inst.id];
            const st = state.stats[inst.id];
            const status =
              running && st?.connected
                ? "online"
                : running
                  ? "starting"
                  : "stopped";
            const statusText =
              status === "online" ? "● 在线" : status === "starting" ? "● 启动中" : "● 停止";
            return (
              <div
                key={inst.id}
                className={`instance-card ${selectedId === inst.id ? "active" : ""}`}
                onClick={() => setSelectedId(inst.id)}
              >
                <div className="row">
                  <div>
                    <div>{inst.name}</div>
                    <div className="muted">{TYPE_LABELS[inst.type]}</div>
                  </div>
                  <span className={`status ${status}`}>{statusText}</span>
                </div>
                <div className="row" style={{ marginTop: "0.45rem" }}>
                  <button
                    className="btn"
                    onClick={(e) => {
                      e.stopPropagation();
                      void withState(() => api.startInstance(inst.id), undefined, {
                        syncDraft: false,
                      });
                    }}
                  >
                    ▶
                  </button>
                  <button
                    className="btn"
                    onClick={(e) => {
                      e.stopPropagation();
                      void withState(() => api.stopInstance(inst.id), undefined, {
                        syncDraft: false,
                      });
                    }}
                  >
                    ■
                  </button>
                  <button
                    className="btn"
                    onClick={(e) => {
                      e.stopPropagation();
                      void withState(() => api.cloneInstance(inst.id), "已复制").then((s) => {
                        if (s) {
                          const last = s.profile.instances[s.profile.instances.length - 1];
                          setSelectedId(last?.id ?? null);
                        }
                      });
                    }}
                  >
                    复制
                  </button>
                  <button
                    className="btn danger"
                    onClick={(e) => {
                      e.stopPropagation();
                      void withState(() => api.removeInstance(inst.id), "已删除").then((s) => {
                        if (selectedId === inst.id) {
                          setSelectedId(s?.profile.instances[0]?.id ?? null);
                        }
                      });
                    }}
                  >
                    删除
                  </button>
                </div>
              </div>
            );
          })}
        </aside>

        <main className="main">
          <section className="panel" style={{ overflow: "auto" }}>
            {!draft ? (
              <div className="empty">
                <h2>选择左侧实例，或新建 MQTT / Modbus 仿真</h2>
                <p className="muted">支持多实例同时运行、自定义数据、按规则随机/波形/递增生成。</p>
              </div>
            ) : (
              <Editor
                draft={draft}
                setDraft={setDraft}
                running={selectedRunning}
                connected={!!selectedStats?.connected}
                onStart={() =>
                  withState(() => api.startInstance(draft.id), "已启动", { syncDraft: false })
                }
                onStop={() =>
                  withState(() => api.stopInstance(draft.id), undefined, { syncDraft: false })
                }
                onApply={applyDraft}
                onTrigger={() =>
                  api.triggerOnce(draft.id).then(
                    () => notify("已触发一次"),
                    (e) => notify(String(e), true),
                  )
                }
              />
            )}
          </section>

          <section className="panel log-panel">
            <div className="log-toolbar">
              <div>
                <p className="section-title">运行监视</p>
                <div className="stats">
                  <span>
                    运行 {totals.running}/{state.profile.instances.length}
                  </span>
                  <span>发送 {totals.sent}</span>
                  <span>接收 {totals.received}</span>
                  <span className="err">错误 {totals.errors}</span>
                </div>
              </div>
              <div className="row">
                <select value={logKind} onChange={(e) => setLogKind(e.target.value)}>
                  <option value="all">全部</option>
                  <option value="error">仅错误</option>
                  <option value="publish">发布</option>
                  <option value="read">读取</option>
                  <option value="write">写入</option>
                  <option value="message">订阅消息</option>
                </select>
                <button className="btn ghost" onClick={() => setLogs([])}>
                  清空
                </button>
              </div>
            </div>
            <div className="log-view">{logs.join("\n") || "等待事件…"}</div>
          </section>
        </main>
      </div>

      {toast && <div className={`toast ${toast.error ? "error" : ""}`}>{toast.text}</div>}
    </div>
  );
}

function Editor({
  draft,
  setDraft,
  running,
  connected,
  onStart,
  onStop,
  onApply,
  onTrigger,
}: {
  draft: SimInstance;
  setDraft: (v: SimInstance) => void;
  running: boolean;
  connected: boolean;
  onStart: () => void;
  onStop: () => void;
  onApply: () => void;
  onTrigger: () => void;
}) {
  return (
    <>
      <div className="editor-head">
        <div>
          <label className="field" style={{ minWidth: 280 }}>
            实例名称
            <input
              value={draft.name}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            />
          </label>
          <p className="muted">
            {TYPE_LABELS[draft.type]} · ID {draft.id}
          </p>
        </div>
        <div className="row">
          <span className={`badge ${running ? "" : "off"}`}>
            {running ? (connected ? "运行中 / 在线" : "运行中") : "已停止"}
          </span>
          {running ? (
            <>
              <button className="btn" onClick={onStop}>
                停止
              </button>
              <button className="btn primary" onClick={onTrigger}>
                立即发送/轮询一次
              </button>
            </>
          ) : (
            <button className="btn primary" onClick={onStart}>
              启动此实例
            </button>
          )}
          <button className="btn" onClick={onApply}>
            应用配置（运行中将重启）
          </button>
        </div>
      </div>

      <label className="field">
        备注
        <textarea
          value={draft.note}
          onChange={(e) => setDraft({ ...draft, note: e.target.value })}
        />
      </label>
      <label className="field" style={{ marginTop: "0.5rem", maxWidth: 120 }}>
        启用
        <input
          type="checkbox"
          checked={draft.enabled}
          onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
        />
      </label>

      {draft.type === "mqtt_client" && draft.mqtt && (
        <MqttEditor
          mqtt={draft.mqtt}
          onChange={(mqtt) => setDraft({ ...draft, mqtt })}
        />
      )}
      {draft.type === "modbus_tcp_client" && draft.modbus_client && (
        <ModbusClientEditor
          cfg={draft.modbus_client}
          onChange={(modbus_client) => setDraft({ ...draft, modbus_client })}
        />
      )}
      {draft.type === "modbus_tcp_slave" && draft.modbus_slave && (
        <ModbusSlaveEditor
          cfg={draft.modbus_slave}
          onChange={(modbus_slave) => setDraft({ ...draft, modbus_slave })}
        />
      )}
    </>
  );
}

function defaultDataField(name = "value"): DataField {
  return {
    name,
    type: "uint16",
    mode: "const",
    value: 0,
    min: 0,
    max: 65535,
    start: 0,
    step: 1,
    period: 60,
    decimals: 0,
    values: [],
    weights: [],
    template: "",
    probability: 0.5,
    wrap: true,
    hex_bytes: 4,
  };
}

function FieldEditors({
  fields,
  onChange,
  allowAddRemove = true,
}: {
  fields: DataField[];
  onChange: (fields: DataField[]) => void;
  /** 从站寄存器等场景只有一个 field，禁止增删条数 */
  allowAddRemove?: boolean;
}) {
  const patch = (idx: number, partial: Partial<DataField>) => {
    onChange(fields.map((f, i) => (i === idx ? { ...f, ...partial } : f)));
  };

  return (
    <>
      {fields.map((field, idx) => (
        <div className="card" key={idx}>
          <div className="field-grid">
            <label className="field">
              字段名
              <input
                value={field.name}
                onChange={(e) => patch(idx, { name: e.target.value })}
              />
            </label>
            <label className="field">
              类型
              <select
                value={field.type}
                onChange={(e) => patch(idx, { type: e.target.value as DataField["type"] })}
              >
                {FIELD_TYPES.map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              生成模式
              <select
                value={field.mode}
                onChange={(e) => patch(idx, { mode: e.target.value as GenMode })}
              >
                {(Object.keys(MODE_LABELS) as GenMode[]).map((m) => (
                  <option key={m} value={m}>
                    {MODE_LABELS[m]}
                  </option>
                ))}
              </select>
            </label>

            {field.type === "bool" && field.mode === "const" ? (
              <label className="field">
                常量 value
                <select
                  value={field.value === true ? "true" : field.value === false ? "false" : ""}
                  onChange={(e) => {
                    const v = e.target.value;
                    patch(idx, {
                      value: v === "" ? null : v === "true",
                    });
                  }}
                >
                  <option value="">（空 / null）</option>
                  <option value="true">true (1)</option>
                  <option value="false">false (0)</option>
                </select>
              </label>
            ) : (
              <label className="field">
                常量 value
                <input
                  placeholder="const 模式使用，如 1 / true / ok"
                  value={formatConstValue(field.value)}
                  onChange={(e) =>
                    patch(idx, { value: parseConstValue(e.target.value, field.type) })
                  }
                />
              </label>
            )}

            <label className="field">
              起始 start
              <input
                type="number"
                value={field.start ?? 0}
                onChange={(e) =>
                  patch(idx, { start: parseNumberInput(e.target.value, field.start ?? 0) })
                }
              />
            </label>
            <label className="field">
              最小值 min
              <input
                type="number"
                value={field.min ?? ""}
                onChange={(e) =>
                  patch(idx, {
                    min: e.target.value === "" ? null : parseNumberInput(e.target.value, 0),
                  })
                }
              />
            </label>
            <label className="field">
              最大值 max
              <input
                type="number"
                value={field.max ?? ""}
                onChange={(e) =>
                  patch(idx, {
                    max: e.target.value === "" ? null : parseNumberInput(e.target.value, 0),
                  })
                }
              />
            </label>
            <label className="field">
              步长 step
              <input
                type="number"
                value={field.step ?? 1}
                onChange={(e) =>
                  patch(idx, { step: parseNumberInput(e.target.value, field.step ?? 1) })
                }
              />
            </label>
            <label className="field">
              周期 period(秒)
              <input
                type="number"
                value={field.period ?? 60}
                onChange={(e) =>
                  patch(idx, { period: parseNumberInput(e.target.value, field.period ?? 60) })
                }
              />
            </label>
            <label className="field">
              均值 mean
              <input
                type="number"
                value={field.mean ?? ""}
                onChange={(e) =>
                  patch(idx, {
                    mean: e.target.value === "" ? null : parseNumberInput(e.target.value, 0),
                  })
                }
              />
            </label>
            <label className="field">
              标准差 std
              <input
                type="number"
                value={field.std ?? ""}
                onChange={(e) =>
                  patch(idx, {
                    std: e.target.value === "" ? null : parseNumberInput(e.target.value, 0),
                  })
                }
              />
            </label>
            <label className="field">
              小数位 decimals
              <input
                type="number"
                value={field.decimals ?? ""}
                onChange={(e) =>
                  patch(idx, {
                    decimals:
                      e.target.value === ""
                        ? null
                        : parseNumberInput(e.target.value, field.decimals ?? 0),
                  })
                }
              />
            </label>
            <label className="field">
              概率 probability
              <input
                type="number"
                step="0.01"
                min={0}
                max={1}
                value={field.probability ?? 0.5}
                onChange={(e) =>
                  patch(idx, {
                    probability: parseNumberInput(e.target.value, field.probability ?? 0.5),
                  })
                }
              />
            </label>
            <label className="field">
              hex 字节数
              <input
                type="number"
                value={field.hex_bytes ?? 4}
                onChange={(e) =>
                  patch(idx, {
                    hex_bytes: parseNumberInput(e.target.value, field.hex_bytes ?? 4),
                  })
                }
              />
            </label>
            <label className="field">
              递增回绕 wrap
              <input
                type="checkbox"
                checked={field.wrap ?? true}
                onChange={(e) => patch(idx, { wrap: e.target.checked })}
              />
            </label>
            <label className="field">
              模板 template
              <input
                placeholder="template / timestamp 格式"
                value={field.template ?? ""}
                onChange={(e) => patch(idx, { template: e.target.value })}
              />
            </label>
            <label className="field">
              枚举 values
              <input
                placeholder="逗号分隔，如 ok,warn,fault 或 true,false"
                value={(field.values ?? [])
                  .map((v) => formatConstValue(v))
                  .join(",")}
                onChange={(e) => {
                  const parts = e.target.value
                    .split(",")
                    .map((x) => x.trim())
                    .filter(Boolean)
                    .map((x) => parseConstValue(x, field.type));
                  patch(idx, { values: parts });
                }}
              />
            </label>
            <label className="field">
              权重 weights
              <input
                placeholder="与 values 等长，如 0.9,0.08,0.02"
                value={(field.weights ?? []).join(",")}
                onChange={(e) => {
                  const weights = e.target.value
                    .split(",")
                    .map((x) => x.trim())
                    .filter(Boolean)
                    .map((x) => Number(x))
                    .filter((n) => Number.isFinite(n));
                  patch(idx, { weights });
                }}
              />
            </label>
          </div>
          {allowAddRemove && (
            <div className="row" style={{ marginTop: "0.45rem" }}>
              <button
                className="btn danger"
                onClick={() => onChange(fields.filter((_, i) => i !== idx))}
              >
                删除字段
              </button>
            </div>
          )}
        </div>
      ))}
      {allowAddRemove && (
        <button
          className="btn"
          onClick={() =>
            onChange([
              ...fields,
              defaultDataField(`field${fields.length + 1}`),
            ])
          }
        >
          添加字段
        </button>
      )}
    </>
  );
}

function MqttEditor({
  mqtt,
  onChange,
}: {
  mqtt: NonNullable<SimInstance["mqtt"]>;
  onChange: (m: NonNullable<SimInstance["mqtt"]>) => void;
}) {
  return (
    <>
      <p className="section-title">连接</p>
      <div className="field-grid">
        <label className="field">
          Broker Host
          <input value={mqtt.host} onChange={(e) => onChange({ ...mqtt, host: e.target.value })} />
        </label>
        <label className="field">
          Port
          <input
            type="number"
            value={mqtt.port}
            onChange={(e) => onChange({ ...mqtt, port: Number(e.target.value) })}
          />
        </label>
        <label className="field">
          Client ID
          <input
            value={mqtt.client_id}
            onChange={(e) => onChange({ ...mqtt, client_id: e.target.value })}
          />
        </label>
        <label className="field">
          Username
          <input
            value={mqtt.username}
            onChange={(e) => onChange({ ...mqtt, username: e.target.value })}
          />
        </label>
        <label className="field">
          Password
          <input
            type="password"
            value={mqtt.password}
            onChange={(e) => onChange({ ...mqtt, password: e.target.value })}
          />
        </label>
        <label className="field">
          Protocol
          <select
            value={mqtt.protocol}
            onChange={(e) => onChange({ ...mqtt, protocol: e.target.value })}
          >
            <option value="MQTTv31">MQTTv31</option>
            <option value="MQTTv311">MQTTv311</option>
            <option value="MQTTv5">MQTTv5</option>
          </select>
        </label>
        <label className="field">
          TLS
          <input
            type="checkbox"
            checked={mqtt.use_tls}
            onChange={(e) => onChange({ ...mqtt, use_tls: e.target.checked })}
          />
        </label>
      </div>

      <p className="section-title">发布规则</p>
      {mqtt.publishes.map((rule, idx) => (
        <div className="card" key={rule.name + idx}>
          <div className="field-grid">
            <label className="field">
              名称
              <input
                value={rule.name}
                onChange={(e) => {
                  const publishes = [...mqtt.publishes];
                  publishes[idx] = { ...rule, name: e.target.value };
                  onChange({ ...mqtt, publishes });
                }}
              />
            </label>
            <label className="field">
              Topic
              <input
                value={rule.topic}
                onChange={(e) => {
                  const publishes = [...mqtt.publishes];
                  publishes[idx] = { ...rule, topic: e.target.value };
                  onChange({ ...mqtt, publishes });
                }}
              />
            </label>
            <label className="field">
              间隔 ms
              <input
                type="number"
                value={rule.interval_ms}
                onChange={(e) => {
                  const publishes = [...mqtt.publishes];
                  publishes[idx] = { ...rule, interval_ms: Number(e.target.value) };
                  onChange({ ...mqtt, publishes });
                }}
              />
            </label>
            <label className="field">
              QoS
              <input
                type="number"
                value={rule.qos}
                onChange={(e) => {
                  const publishes = [...mqtt.publishes];
                  publishes[idx] = { ...rule, qos: Number(e.target.value) };
                  onChange({ ...mqtt, publishes });
                }}
              />
            </label>
            <label className="field">
              Payload
              <select
                value={rule.payload_kind}
                onChange={(e) => {
                  const publishes = [...mqtt.publishes];
                  publishes[idx] = { ...rule, payload_kind: e.target.value };
                  onChange({ ...mqtt, publishes });
                }}
              >
                <option value="json">json</option>
                <option value="text">text</option>
                <option value="hex">hex</option>
              </select>
            </label>
          </div>
          <FieldEditors
            fields={rule.fields}
            onChange={(fields) => {
              const publishes = [...mqtt.publishes];
              publishes[idx] = { ...rule, fields };
              onChange({ ...mqtt, publishes });
            }}
          />
        </div>
      ))}
      <button
        className="btn"
        onClick={() =>
          onChange({
            ...mqtt,
            publishes: [
              ...mqtt.publishes,
              {
                enabled: true,
                name: `rule${mqtt.publishes.length + 1}`,
                topic: "devices/${name}/telemetry",
                qos: 0,
                retain: false,
                interval_ms: 1000,
                jitter_ms: 0,
                payload_kind: "json",
                text_template: "",
                fields: [],
              },
            ],
          })
        }
      >
        添加发布规则
      </button>

      <p className="section-title">订阅</p>
      {mqtt.subscriptions.map((sub, idx) => (
        <div className="card" key={idx}>
          <div className="field-grid">
            <label className="field">
              Topic
              <input
                value={sub.topic}
                onChange={(e) => {
                  const subscriptions = [...mqtt.subscriptions];
                  subscriptions[idx] = { ...sub, topic: e.target.value };
                  onChange({ ...mqtt, subscriptions });
                }}
              />
            </label>
            <label className="field">
              QoS
              <input
                type="number"
                value={sub.qos}
                onChange={(e) => {
                  const subscriptions = [...mqtt.subscriptions];
                  subscriptions[idx] = { ...sub, qos: Number(e.target.value) };
                  onChange({ ...mqtt, subscriptions });
                }}
              />
            </label>
          </div>
        </div>
      ))}
    </>
  );
}

function ModbusClientEditor({
  cfg,
  onChange,
}: {
  cfg: NonNullable<SimInstance["modbus_client"]>;
  onChange: (c: NonNullable<SimInstance["modbus_client"]>) => void;
}) {
  return (
    <>
      <p className="section-title">连接</p>
      <div className="field-grid">
        <label className="field">
          Host
          <input value={cfg.host} onChange={(e) => onChange({ ...cfg, host: e.target.value })} />
        </label>
        <label className="field">
          Port
          <input
            type="number"
            value={cfg.port}
            onChange={(e) => onChange({ ...cfg, port: Number(e.target.value) })}
          />
        </label>
        <label className="field">
          Unit ID
          <input
            type="number"
            value={cfg.unit_id}
            onChange={(e) => onChange({ ...cfg, unit_id: Number(e.target.value) })}
          />
        </label>
      </div>
      <p className="section-title">操作</p>
      {cfg.operations.map((op, idx) => (
        <div className="card" key={op.name + idx}>
          <div className="field-grid">
            <label className="field">
              名称
              <input
                value={op.name}
                onChange={(e) => {
                  const operations = [...cfg.operations];
                  operations[idx] = { ...op, name: e.target.value };
                  onChange({ ...cfg, operations });
                }}
              />
            </label>
            <label className="field">
              功能码
              <select
                value={op.function}
                onChange={(e) => {
                  const operations = [...cfg.operations];
                  operations[idx] = {
                    ...op,
                    function: e.target.value as (typeof MODBUS_FNS)[number],
                  };
                  onChange({ ...cfg, operations });
                }}
              >
                {MODBUS_FNS.map((f) => (
                  <option key={f} value={f}>
                    {f}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              地址
              <input
                type="number"
                value={op.address}
                onChange={(e) => {
                  const operations = [...cfg.operations];
                  operations[idx] = { ...op, address: Number(e.target.value) };
                  onChange({ ...cfg, operations });
                }}
              />
            </label>
            <label className="field">
              数量
              <input
                type="number"
                value={op.count}
                onChange={(e) => {
                  const operations = [...cfg.operations];
                  operations[idx] = { ...op, count: Number(e.target.value) };
                  onChange({ ...cfg, operations });
                }}
              />
            </label>
            <label className="field">
              间隔 ms
              <input
                type="number"
                value={op.interval_ms}
                onChange={(e) => {
                  const operations = [...cfg.operations];
                  operations[idx] = { ...op, interval_ms: Number(e.target.value) };
                  onChange({ ...cfg, operations });
                }}
              />
            </label>
          </div>
          <FieldEditors
            fields={op.fields}
            onChange={(fields) => {
              const operations = [...cfg.operations];
              operations[idx] = { ...op, fields };
              onChange({ ...cfg, operations });
            }}
          />
          <div className="row" style={{ marginTop: "0.45rem" }}>
            <button
              className="btn danger"
              onClick={() =>
                onChange({
                  ...cfg,
                  operations: cfg.operations.filter((_, i) => i !== idx),
                })
              }
            >
              删除操作
            </button>
          </div>
        </div>
      ))}
      <button
        className="btn"
        onClick={() => {
          const n = cfg.operations.length + 1;
          onChange({
            ...cfg,
            operations: [
              ...cfg.operations,
              {
                enabled: true,
                name: `op-${n}`,
                function: "read_holding_registers",
                address: 0,
                count: 10,
                interval_ms: 1000,
                jitter_ms: 0,
                data_type: "uint16",
                byte_order: "ABCD",
                fields: [],
              },
            ],
          });
        }}
      >
        添加操作
      </button>
    </>
  );
}

function defaultRegister(address = 0): ModbusRegisterDef {
  return {
    enabled: true,
    name: `reg-${address}`,
    table: "holding_registers",
    address,
    data_type: "uint16",
    byte_order: "ABCD",
    interval_ms: 1000,
    jitter_ms: 0,
    field: defaultDataField(`reg-${address}`),
  };
}

function ModbusSlaveEditor({
  cfg,
  onChange,
}: {
  cfg: NonNullable<SimInstance["modbus_slave"]>;
  onChange: (c: NonNullable<SimInstance["modbus_slave"]>) => void;
}) {
  const registers = cfg.registers ?? [];

  const patchReg = (idx: number, patch: Partial<ModbusRegisterDef>) => {
    const next = registers.map((r, i) => (i === idx ? { ...r, ...patch } : r));
    onChange({ ...cfg, registers: next });
  };

  const addRegister = () => {
    const used = new Set(registers.map((r) => r.address));
    let address = 0;
    while (used.has(address)) address += 1;
    onChange({
      ...cfg,
      registers: [...registers, defaultRegister(address)],
    });
  };

  return (
    <>
      <p className="section-title">监听</p>
      <div className="field-grid">
        <label className="field">
          Bind Host
          <input value={cfg.host} onChange={(e) => onChange({ ...cfg, host: e.target.value })} />
        </label>
        <label className="field">
          Port
          <input
            type="number"
            value={cfg.port}
            onChange={(e) =>
              onChange({ ...cfg, port: parseNumberInput(e.target.value, cfg.port) })
            }
          />
        </label>
        <label className="field">
          Unit ID
          <input
            type="number"
            value={cfg.unit_id}
            onChange={(e) =>
              onChange({ ...cfg, unit_id: parseNumberInput(e.target.value, cfg.unit_id) })
            }
          />
        </label>
      </div>
      <div className="row" style={{ justifyContent: "space-between", marginTop: "0.75rem" }}>
        <p className="section-title" style={{ margin: 0 }}>
          寄存器映射
        </p>
        <button className="btn primary" type="button" onClick={addRegister}>
          添加寄存器
        </button>
      </div>
      {!registers.length && (
        <p className="muted" style={{ margin: "0.5rem 0" }}>
          还没有寄存器映射，点击「添加寄存器」。
        </p>
      )}
      {registers.map((reg, idx) => (
        <div className="card" key={idx}>
          <div className="field-grid">
            <label className="field">
              名称
              <input
                value={reg.name}
                onChange={(e) => patchReg(idx, { name: e.target.value })}
              />
            </label>
            <label className="field">
              表
              <select
                value={reg.table}
                onChange={(e) =>
                  patchReg(idx, { table: e.target.value as (typeof TABLES)[number] })
                }
              >
                {TABLES.map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              地址
              <input
                type="number"
                value={reg.address}
                onChange={(e) =>
                  patchReg(idx, { address: parseNumberInput(e.target.value, reg.address) })
                }
              />
            </label>
            <label className="field">
              数据类型
              <select
                value={reg.data_type}
                onChange={(e) => {
                  const data_type = e.target.value as DataField["type"];
                  patchReg(idx, {
                    data_type,
                    field: { ...reg.field, type: data_type, name: reg.field.name || reg.name },
                  });
                }}
              >
                {FIELD_TYPES.map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              字节序
              <select
                value={reg.byte_order}
                onChange={(e) =>
                  patchReg(idx, {
                    byte_order: e.target.value as (typeof BYTE_ORDERS)[number],
                  })
                }
              >
                {BYTE_ORDERS.map((o) => (
                  <option key={o} value={o}>
                    {o}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              间隔 ms
              <input
                type="number"
                value={reg.interval_ms}
                onChange={(e) =>
                  patchReg(idx, {
                    interval_ms: parseNumberInput(e.target.value, reg.interval_ms),
                  })
                }
              />
            </label>
            <label className="field">
              抖动 ms
              <input
                type="number"
                value={reg.jitter_ms}
                onChange={(e) =>
                  patchReg(idx, {
                    jitter_ms: parseNumberInput(e.target.value, reg.jitter_ms),
                  })
                }
              />
            </label>
            <label className="field">
              启用
              <input
                type="checkbox"
                checked={reg.enabled}
                onChange={(e) => patchReg(idx, { enabled: e.target.checked })}
              />
            </label>
          </div>
          <FieldEditors
            allowAddRemove={false}
            fields={[reg.field ?? defaultDataField(reg.name)]}
            onChange={(fields) => {
              const field = fields[0] ?? defaultDataField(reg.name);
              patchReg(idx, {
                field,
                data_type: field.type,
              });
            }}
          />
          <div className="row" style={{ marginTop: "0.45rem" }}>
            <button
              className="btn danger"
              type="button"
              onClick={() =>
                onChange({
                  ...cfg,
                  registers: registers.filter((_, i) => i !== idx),
                })
              }
            >
              删除此寄存器
            </button>
          </div>
        </div>
      ))}
      <button className="btn" type="button" onClick={addRegister} style={{ marginTop: "0.5rem" }}>
        添加寄存器
      </button>
    </>
  );
}
