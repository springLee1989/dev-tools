# 架构说明

```
方案 JSON  ──► Engine ──► MqttClientRunner / ModbusTcpClientRunner / ModbusTcpSlaveRunner
                 │
                 ▼
            事件队列 ──► NiceGUI（日志 / 状态）或 CLI stdout
```

- **模型** `devicesim.core.models`：Pydantic v2，方案文件的唯一来源。
- **生成器** `devicesim.core.data_gen`：字段状态机（递增、正弦相位、游走当前位置）。
- **运行器**：每个实例一条后台线程（从站线程内再开 asyncio loop）。GUI 线程只消费队列。
- **界面** `devicesim.ui`：NiceGUI + Quasar。开发时走浏览器，打包时可 `native=True`。

核心协议库：`paho-mqtt`、`pymodbus`。不要在 UI 模块里直接连 socket，便于无界面复用。
