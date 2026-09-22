# 编译与打包（macOS / Windows）

目标：在两台主流 PC 上用同一套源码产出可双击运行的应用，不依赖用户先装 Python。

## 1. 开发运行（不打包）

```bash
python3 -m venv .venv
source .venv/bin/activate          # Windows: .venv\Scripts\activate
pip install -U pip
pip install -e ".[dev]"
devicesim
```

原生窗口（需可选依赖）：

```bash
pip install -e ".[dev,desktop]"
devicesim gui --native
```

建议使用 **Python 3.12** 打包。3.11 也可以。请不要用 Windows Store 精简版 Python。

## 2. macOS 本地出包

依赖：Xcode Command Line Tools、Python 3.12。

```bash
bash scripts/build-macos.sh
```

产物：

- `dist/DeviceSim` 命令行可执行文件
- `dist/DeviceSim.app`（spec 配置为 onedir + windowed）

首次在未签名机器上打开，若被 Gatekeeper 拦截：系统设置 → 隐私与安全性 → 仍要打开。正式分发需要 Apple 开发者证书公证，本仓库不绑定证书。

## 3. Windows 本地出包

在 PowerShell 中：

```powershell
py -3.12 -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -e ".[dev]"
powershell -ExecutionPolicy Bypass -File scripts\build-windows.ps1
```

产物：`dist/DeviceSim/DeviceSim.exe`。把整个 `DeviceSim` 文件夹打 zip 即可分发。

若杀毒软件误报 PyInstaller 包，属于常见情况，用公司内部证书签名可缓解。不要关实时防护去「解决」误报。

## 4. GitHub Actions

推送 tag 或手动 `workflow_dispatch` 会跑 `.github/workflows/build.yml`：

- `macos-latest` → 上传 `DeviceSim-macos` artifact
- `windows-latest` → 上传 `DeviceSim-windows` artifact

测试工作流 `.github/workflows/test.yml` 在 Ubuntu / macOS / Windows 上跑 `pytest`。

## 5. PyInstaller 说明

入口：`src/devicesim/__main__.py`  
配置：`packaging/devicesim.spec`

必须收集的数据：

- `nicegui` 静态资源与 metadata
- `devicesim` 包
- `paho` / `pymodbus` 子模块（hiddenimports）

改 UI 或新增依赖后，若打包应用启动报 `ModuleNotFoundError`，把模块名加入 spec 的 `hiddenimports`。

## 6. 常见问题

**端口 18765 被占用**  
`devicesim gui --port 18766`

**本机没有 Tk**  
本工具 GUI 使用 NiceGUI，不依赖 Tkinter。Homebrew Python 缺 `_tkinter` 不影响。

**Modbus 从站 bind 失败**  
换端口；macOS/Windows 上 502 常需管理员权限，示例一律用 5020。

**MQTT 连不上**  
先确认 Broker 是否监听 `127.0.0.1:1883`。本工具不会内置 Broker，避免和现场 Mosquitto 抢端口。
