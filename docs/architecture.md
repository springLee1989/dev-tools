# 架构说明

```
方案 JSON  ──► Engine(Rust) ──► MqttClientRunner / ModbusTcpClientRunner / ModbusTcpSlaveRunner
                 │
                 ▼
            Tauri Event(sim-event) ──► React UI（日志 / 状态）
                 │
                 └── devicesim-cli stdout
```

- **模型** `src-tauri/src/models.rs`：Serde + schemars，方案文件的唯一来源，兼容原 Python 版 JSON。
- **生成器** `src-tauri/src/data_gen.rs`：字段状态机（递增、正弦相位、游走当前位置）。
- **运行器**：每个实例一个 Tokio 任务；从站内嵌轻量 Modbus TCP 服务。
- **界面** `src/`：React + Vite，开发时由 Vite 提供页面，打包后由 Tauri WebView 加载。

核心协议库：`rumqttc`（MQTT）、自研 / `tokio-modbus`（Modbus TCP 客户端）。不要在前端直接连 socket，便于无界面复用。
