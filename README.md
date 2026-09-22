# DeviceSim

PC 端 **MQTT 客户端 / Modbus TCP 客户端 / Modbus TCP 从站** 多实例仿真工具，用来模拟支持 MQTT、Modbus TCP 的设备与主站。

可在 **macOS** 与 **Windows** 上开发、运行，并用 PyInstaller 打成独立应用。

## 能做什么

- **多实例同时运行**：任意数量的 MQTT 客户端、Modbus TCP 客户端（主站）、Modbus TCP 从站（被仿真设备）并行工作。
- **数据自定义**：JSON 载荷字段、Modbus 寄存器映射、主题、功能码、周期、QoS 等均可配置。
- **按规则产生数据**：常量、均匀随机、高斯、递增、正弦波、枚举加权、随机游走、时间戳、UUID、字符串模板。
- **仿真工具常用能力**：方案保存/导入/导出、自动重连、TLS、遗嘱消息、立即发送一次、运行日志与统计、无界面 CLI、JSON Schema。

> 设备侧在现场通常是 MQTT **客户端** + Modbus TCP **从站（Slave）**。本工具按你的需求实现了两种客户端，并额外提供从站模式，这样才能完整仿真一台可被 SCADA/主站读取的设备。

## 环境

- Python **3.11+**（打包推荐 3.12）
- macOS 12+ 或 Windows 10+
- MQTT 仿真需要可连的 Broker（如 Mosquitto）；Modbus 从站+客户端可在本机互相对打，无需外部设备。

## 安装与启动

```bash
cd dev-tools
python3 -m venv .venv

# macOS / Linux
source .venv/bin/activate

# Windows
.venv\Scripts\activate

pip install -e ".[dev]"
devicesim
```

浏览器会打开 `http://127.0.0.1:18765`（也可加 `--native` 使用系统窗口）：

```bash
devicesim gui --native
devicesim gui --profile examples/mqtt-temperature-sensor.json
```

无界面批量跑方案：

```bash
devicesim run examples/mixed-gateway.json
devicesim validate examples/modbus-tcp-slave.json
devicesim schema > profile.schema.json
```

## 界面用法（最短路径）

1. 右上角 **新建** → 选择 MQTT 客户端 / Modbus TCP 客户端 / Modbus TCP 从站。
2. 填写连接参数（Broker 地址、Modbus 主机/端口等）。
3. 在发布规则或寄存器映射里配置数据字段与生成模式。
4. 点实例上的播放按钮，或 **全部启动**。
5. 底部日志查看发布、读写、订阅与错误；左侧状态点为青绿色表示已连接。
6. **保存** 方案为 JSON，可拷到另一台电脑继续用。

仓库 `examples/` 里有可直接加载的方案：

| 文件 | 说明 |
| --- | --- |
| `mqtt-temperature-sensor.json` | 单个温湿度 MQTT 传感器 |
| `mqtt-multi-devices.json` | 三个 MQTT 客户端并发 |
| `modbus-tcp-slave.json` | 虚拟泵设备（从站，端口 5020） |
| `modbus-tcp-client.json` | 主站轮询 5020 并写设定值 |
| `mixed-gateway.json` | MQTT + 从站 + 主站同时跑 |

更细的操作见 [docs/user-guide.md](docs/user-guide.md)，数据规则见 [docs/data-rules.md](docs/data-rules.md)，打包见 [docs/build.md](docs/build.md)。

## 开发与测试

```bash
pytest -q
```

## 打包

```bash
# macOS
bash scripts/build-macos.sh

# Windows PowerShell
powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1
```

产物在 `dist/`。GitHub Actions 工作流 `.github/workflows/build.yml` 可在 macOS / Windows runner 上自动出包。

## 协议范围

| 类型 | 角色 | 能力 |
| --- | --- | --- |
| MQTT 客户端 | 设备 / 网关 | 3.1 / 3.1.1 / 5，用户名密码，TLS，Birth/LWT，多主题周期发布，订阅 |
| Modbus TCP 客户端 | 主站 | 功能码 01/02/03/04/05/06/15/16，周期读/写，字节序 ABCD/CDAB/BADC/DCBA |
| Modbus TCP 从站 | 设备 | 监听端口，刷新 Coils / Discrete Inputs / Holding / Input 寄存器 |

## 许可证

MIT
