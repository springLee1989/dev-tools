from __future__ import annotations

import json
import os
import queue
import sys
import time
from datetime import datetime
from pathlib import Path

from nicegui import app as ng_app
from nicegui import ui

from devicesim.core.engine import Engine
from devicesim.core.events import SimEvent
from devicesim.core.models import TYPE_LABELS, InstanceType
from devicesim.ui.editors import render_modbus_client, render_modbus_slave, render_mqtt
from devicesim.version import __version__

engine = Engine()
events: queue.Queue[SimEvent] = queue.Queue()
engine.set_emit(events.put)


class State:
    selected_id: str | None = None
    path: str = str(Path.home() / ".devicesim" / "profile.json")
    log_kind: str = "all"


state = State()
_status_labels: dict[str, ui.label] = {}
_stat_bar: dict[str, ui.label] = {}
log_view = None


def _on_event_line(event: SimEvent) -> str:
    ts = datetime.fromtimestamp(event.ts).strftime("%H:%M:%S")
    return f"{ts}  {event.level.upper():<5}  {event.instance_name:<16}  {event.kind:<10}  {event.message}"


def _select(instance_id: str | None) -> None:
    state.selected_id = instance_id
    editor_panel.refresh()
    list_panel.refresh()


def _new(type_: InstanceType) -> None:
    inst = engine.add_default(type_)
    _select(inst.id)
    ui.notify(f"已创建 {inst.name}", type="positive")


def _start_selected() -> None:
    if not state.selected_id:
        ui.notify("请先选择实例", type="warning")
        return
    try:
        engine.start_instance(state.selected_id)
        ui.notify("已启动", type="positive")
        list_panel.refresh()
        editor_panel.refresh()
    except Exception as exc:
        ui.notify(str(exc), type="negative")


def _stop_selected() -> None:
    if not state.selected_id:
        return
    engine.stop_instance(state.selected_id)
    list_panel.refresh()
    editor_panel.refresh()


def _start_all() -> None:
    engine.start_all()
    ui.notify("已启动全部启用实例", type="positive")
    list_panel.refresh()


def _stop_all() -> None:
    engine.stop_all()
    ui.notify("已全部停止", type="info")
    list_panel.refresh()


def _save(path: str | None = None) -> None:
    target = Path(path or state.path)
    try:
        engine.save_profile(target)
        state.path = str(target)
        ui.notify(f"已保存 {target}", type="positive")
    except Exception as exc:
        ui.notify(f"保存失败: {exc}", type="negative")


def _load(path: str | None = None) -> None:
    target = Path(path or state.path)
    try:
        engine.load_profile(target)
        state.path = str(target)
        state.selected_id = engine.profile.instances[0].id if engine.profile.instances else None
        list_panel.refresh()
        editor_panel.refresh()
        header_meta.refresh()
        ui.notify(f"已加载 {target}", type="positive")
    except Exception as exc:
        ui.notify(f"加载失败: {exc}", type="negative")


def _export() -> None:
    ui.download(engine.dump_profile().encode("utf-8"), filename=f"{engine.profile.name or 'profile'}.json")


def build_page() -> None:
    ui.dark_mode().enable()
    ui.colors(
        primary="#14b8a6",
        secondary="#0f766e",
        accent="#22d3ee",
        dark="#070b14",
        dark_page="#070b14",
        positive="#14b8a6",
        negative="#f43f5e",
        info="#38bdf8",
        warning="#fbbf24",
    )
    ui.query("body").style("background: #070b14")
    ui.add_head_html(
        """
        <style>
          .q-drawer, .nicegui-content { background: #070b14 !important; }
          .q-uploader { min-height: 42px !important; }
          .q-uploader__list { max-height: 0; overflow: hidden; }
          .nicegui-log { background: #020617; }
        </style>
        """
    )

    with ui.header().classes("items-center justify-between px-4 bg-slate-950 border-b border-slate-800"):
        with ui.row().classes("items-center gap-3"):
            ui.icon("sensors").classes("text-teal-400 text-3xl")
            with ui.column().classes("gap-0"):
                ui.label("DeviceSim").classes("text-lg font-semibold tracking-wide")
                header_meta()
        with ui.row().classes("items-center gap-2"):
            ui.button("全部启动", icon="play_arrow", on_click=_start_all).props("unelevated")
            ui.button("全部停止", icon="stop", on_click=_stop_all).props("outline color=negative")
            with ui.button("新建", icon="add").props("outline"):
                with ui.menu():
                    ui.menu_item("MQTT 客户端", lambda: _new(InstanceType.MQTT_CLIENT))
                    ui.menu_item("Modbus TCP 客户端", lambda: _new(InstanceType.MODBUS_TCP_CLIENT))
                    ui.menu_item("Modbus TCP 从站（设备）", lambda: _new(InstanceType.MODBUS_TCP_SLAVE))
            ui.button("打开", icon="folder_open", on_click=lambda: _load()).props("flat")
            ui.button("保存", icon="save", on_click=lambda: _save()).props("flat")
            ui.button("导出", icon="download", on_click=_export).props("flat")
            ui.button("帮助", icon="help_outline", on_click=lambda: help_dialog.open()).props("flat")

    with ui.left_drawer(value=True, bordered=True, bottom_corner=True).classes(
        "bg-slate-950 border-slate-800 px-2"
    ).props("width=340"):
        list_panel()

    with ui.column().classes("w-full h-full p-3 gap-3"):
        with ui.card().classes("w-full flex-1 ds-card bg-slate-950/80 p-0"):
            with ui.scroll_area().classes("w-full").style("height: calc(100vh - 360px)"):
                with ui.column().classes("p-4 w-full"):
                    editor_panel()
        with ui.card().classes("w-full ds-card bg-slate-950/80"):
            stats_row()
            log_panel()

    help_dialog = _help_dialog()
    ui.timer(0.25, _tick)


@ui.refreshable
def header_meta() -> None:
    n = len(engine.profile.instances)
    ui.label(f"MQTT / Modbus TCP 设备仿真器  v{__version__}  ·  {engine.profile.name}  ·  {n} 个实例").classes(
        "text-xs text-slate-400"
    )


@ui.refreshable
def list_panel() -> None:
    _status_labels.clear()
    with ui.column().classes("w-full gap-2 q-pa-sm"):
        ui.label("实例").classes("text-xs uppercase tracking-wider text-slate-400")
        ui.input("方案文件路径").bind_value(state, "path").classes("w-full").props("dense dark")
        with ui.row().classes("w-full gap-1"):
            ui.button("加载", on_click=lambda: _load()).props("dense outline")
            ui.button("保存到此路径", on_click=lambda: _save()).props("dense outline")
        ui.upload(
            label="导入 JSON",
            auto_upload=True,
            on_upload=_on_upload,
        ).props("accept=.json dense flat").classes("w-full")

        if not engine.profile.instances:
            ui.label("还没有实例，点击右上角「新建」。").classes("text-slate-500 text-sm q-pa-md")
            return
        for inst in engine.profile.instances:
            selected = inst.id == state.selected_id
            border = "border-teal-400" if selected else "border-slate-700"
            with ui.card().classes(f"w-full cursor-pointer bg-slate-900 {border} border").on(
                "click", lambda i=inst: _select(i.id)
            ):
                with ui.row().classes("w-full items-center justify-between"):
                    with ui.column().classes("gap-0"):
                        ui.label(inst.name).classes("font-medium")
                        ui.label(TYPE_LABELS[inst.type]).classes("text-xs text-slate-400")
                    with ui.row().classes("items-center gap-1"):
                        lab = ui.label("● 停止").classes("text-xs text-slate-500")
                        _status_labels[inst.id] = lab
                with ui.row().classes("w-full gap-1"):
                    ui.switch("启用").bind_value(inst, "enabled").on(
                        "click", lambda e: e.stop_propagation() if hasattr(e, "stop_propagation") else None
                    )
                    ui.button(icon="play_arrow", on_click=lambda i=inst: _start(i.id)).props(
                        "flat dense color=positive"
                    )
                    ui.button(icon="stop", on_click=lambda i=inst: engine.stop_instance(i.id) or list_panel.refresh()).props(
                        "flat dense"
                    )
                    ui.button(icon="content_copy", on_click=lambda i=inst: _clone(i.id)).props("flat dense")
                    ui.button(icon="delete", on_click=lambda i=inst: _delete(i.id)).props("flat dense color=negative")


def _start(instance_id: str) -> None:
    try:
        engine.start_instance(instance_id)
        list_panel.refresh()
    except Exception as exc:
        ui.notify(str(exc), type="negative")


def _clone(instance_id: str) -> None:
    inst = engine.clone_instance(instance_id)
    if inst:
        _select(inst.id)
        ui.notify(f"已复制 {inst.name}", type="positive")


def _delete(instance_id: str) -> None:
    engine.remove_instance(instance_id)
    if state.selected_id == instance_id:
        state.selected_id = engine.profile.instances[0].id if engine.profile.instances else None
    list_panel.refresh()
    editor_panel.refresh()
    header_meta.refresh()


async def _on_upload(e) -> None:
    try:
        text = await e.file.text()
        engine.load_profile_data(text)
        state.selected_id = engine.profile.instances[0].id if engine.profile.instances else None
        list_panel.refresh()
        editor_panel.refresh()
        header_meta.refresh()
        ui.notify("方案已导入", type="positive")
    except Exception as exc:
        ui.notify(f"导入失败: {exc}", type="negative")


def _clear_log() -> None:
    if log_view is None:
        return
    while log_view.default_slot.children:
        log_view.remove(0)


@ui.refreshable
def editor_panel() -> None:
    inst = engine.get_instance(state.selected_id) if state.selected_id else None
    if inst is None:
        with ui.column().classes("items-center justify-center w-full py-24 text-slate-500"):
            ui.icon("hub").classes("text-6xl text-slate-700")
            ui.label("选择左侧实例，或新建 MQTT / Modbus 仿真客户端")
            ui.label("支持多实例同时运行、自定义数据、按规则随机/波形/递增生成。").classes("text-sm")
        return

    running = engine.is_running(inst.id)
    with ui.row().classes("w-full items-center justify-between"):
        with ui.column().classes("gap-0"):
            ui.input("实例名称").bind_value(inst, "name").classes("text-xl w-80")
            ui.label(TYPE_LABELS[inst.type] + (f"  ·  ID {inst.id}" )).classes("text-xs text-slate-400")
        with ui.row().classes("gap-2"):
            if running:
                ui.badge("运行中").props("color=positive")
                ui.button("停止", icon="stop", on_click=_stop_selected).props("outline")
                ui.button("立即发送/轮询一次", icon="bolt", on_click=_trigger).props("unelevated")
            else:
                ui.badge("已停止").props("color=grey")
                ui.button("启动此实例", icon="play_arrow", on_click=_start_selected).props("unelevated")
            ui.button("应用配置（运行中将重启）", icon="save", on_click=_apply).props("outline")
    ui.textarea("备注").bind_value(inst, "note").classes("w-full").props("dense autogrow")

    def refresh():
        editor_panel.refresh()

    if inst.type == InstanceType.MQTT_CLIENT:
        render_mqtt(inst, refresh)
    elif inst.type == InstanceType.MODBUS_TCP_CLIENT:
        render_modbus_client(inst, refresh)
    else:
        render_modbus_slave(inst, refresh)


def _apply() -> None:
    inst = engine.get_instance(state.selected_id) if state.selected_id else None
    if inst is None:
        return
    engine.replace_instance(inst)
    ui.notify("配置已应用", type="positive")
    list_panel.refresh()
    editor_panel.refresh()


def _trigger() -> None:
    try:
        engine.trigger_once(state.selected_id or "")
    except Exception as exc:
        ui.notify(str(exc), type="warning")


def stats_row() -> None:
    with ui.row().classes("w-full justify-between items-center"):
        ui.label("运行监视").classes("text-xs uppercase tracking-wider text-slate-400")
        with ui.row().classes("gap-6"):
            _stat_bar["run"] = ui.label("运行 0").classes("text-sm")
            _stat_bar["sent"] = ui.label("发送 0").classes("text-sm")
            _stat_bar["recv"] = ui.label("接收 0").classes("text-sm")
            _stat_bar["err"] = ui.label("错误 0").classes("text-sm text-rose-400")


def log_panel() -> None:
    with ui.row().classes("w-full items-center justify-between"):
        ui.label("日志").classes("text-sm text-slate-300")
        with ui.row().classes("items-center gap-2"):
            ui.select(
                {"all": "全部", "error": "仅错误", "publish": "发布", "read": "读取", "write": "写入", "message": "订阅消息"},
                label="过滤",
            ).bind_value(state, "log_kind").props("dense").classes("w-40")
            ui.button("清空", on_click=_clear_log).props("flat dense")
            ui.button("复制最近方案", on_click=_export).props("flat dense")
    global log_view
    log_view = ui.log(max_lines=400).classes("w-full h-40 ds-mono text-xs bg-black/50")


def _tick() -> None:
    if log_view is None:
        return
    drained = 0
    while drained < 80:
        try:
            event = events.get_nowait()
        except queue.Empty:
            break
        drained += 1
        if state.log_kind != "all" and event.kind != state.log_kind and event.level != state.log_kind:
            continue
        log_view.push(_on_event_line(event))

    running = 0
    sent = recv = err = 0
    for inst in engine.profile.instances:
        st = engine.stats_of(inst.id)
        sent += st.sent
        recv += st.received
        err += st.errors
        is_run = engine.is_running(inst.id)
        if is_run:
            running += 1
        lab = _status_labels.get(inst.id)
        if lab is None:
            continue
        if is_run and st.connected:
            lab.set_text("● 在线")
            lab.classes(replace="text-xs text-teal-400")
        elif is_run:
            lab.set_text("● 启动中")
            lab.classes(replace="text-xs text-amber-400")
        else:
            lab.set_text("● 停止")
            lab.classes(replace="text-xs text-slate-500")
    if _stat_bar:
        _stat_bar["run"].set_text(f"运行 {running}/{len(engine.profile.instances)}")
        _stat_bar["sent"].set_text(f"发送 {sent}")
        _stat_bar["recv"].set_text(f"接收 {recv}")
        _stat_bar["err"].set_text(f"错误 {err}")


def _help_dialog():
    with ui.dialog() as dialog, ui.card().classes("w-[720px] max-w-full"):
        ui.label("DeviceSim 使用要点").classes("text-lg")
        ui.markdown(
            """
- **多实例并发**：左侧可同时创建多个 MQTT 客户端、Modbus TCP 客户端、Modbus TCP 从站，分别启动。
- **MQTT**：连接 Broker 后按规则周期发布 JSON/文本；支持 TLS、用户名密码、遗嘱、订阅命令主题。
- **Modbus TCP 客户端**：作为主站周期读/写远端设备（功能码 01/02/03/04/05/06/15/16）。
- **Modbus TCP 从站**：作为被仿真设备监听端口，按规则刷新线圈/寄存器，供真实主站或本工具客户端访问。
- **数据规则**：常量、均匀随机、高斯、递增、正弦、枚举、随机游走、时间戳、UUID、模板。
- **方案文件**：全部配置保存为 JSON，可在 macOS / Windows 间拷贝；也可用 `devicesim run profile.json` 无界面运行。
- 模板占位符：`${name}` `${id}` 以及 JSON 字段名。
            """
        )
        ui.button("关闭", on_click=dialog.close).props("flat")
    return dialog


def run_gui(
    host: str = "127.0.0.1",
    port: int = 18765,
    native: bool = False,
    profile: str | None = None,
) -> None:
    if profile:
        engine.load_profile(profile)
    else:
        default_dir = Path.home() / ".devicesim"
        default_dir.mkdir(parents=True, exist_ok=True)
        last = default_dir / "profile.json"
        if last.exists():
            try:
                engine.load_profile(last)
                state.path = str(last)
            except Exception:
                pass
        if engine.profile_path is None:
            state.path = str(last)

    if state.selected_id is None and engine.profile.instances:
        state.selected_id = engine.profile.instances[0].id

    frozen = bool(getattr(sys, "frozen", False))
    use_native = native or frozen or os.environ.get("DEVICESIM_NATIVE") == "1"
    kwargs: dict = dict(
        title="DeviceSim",
        host=host,
        port=port,
        reload=False,
        show=os.environ.get("DEVICESIM_SHOW", "1") != "0",
        dark=True,
        favicon="📡",
    )
    if use_native:
        kwargs.update(native=True, window_size=(1440, 920), frameless=False)
    try:
        ng_app.on_shutdown(engine.stop_all)
    except Exception:
        pass
    ui.run(build_page, **kwargs)
