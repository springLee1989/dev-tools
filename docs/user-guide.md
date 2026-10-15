# 用户指南

## 1. 概念

一个 **方案（Profile）** 是一份 JSON，里面有若干 **实例（Instance）**。每个实例独立连接、独立计时、独立生成数据，因此可以同时仿真一条产线上的多台设备。

实例类型：

| 类型 | 仿真对象 | 典型用途 |
| --- | --- | --- |
| MQTT 客户端 | 连 Broker 的设备 | 上报遥测、收控制命令 |
| Modbus TCP 客户端 | 主站 / 网关 | 轮询 PLC 或本工具从站 |
| Modbus TCP 从站 | 被寻址的设备 | 给真实 SCADA 或其它主站当假设备 |

## 2. 图形界面

启动后分为三块：

- **左侧**：方案路径、导入 JSON、实例列表（启用开关、启动/停止/复制/删除）。
- **中间**：当前实例的连接参数与数据规则。改完后点 **应用配置**；若实例正在运行会重启以加载新参数。
- **底部**：发送/接收/错误计数与滚动日志。可用过滤器只看错误、发布或读写。

工具栏：

- **全部启动 / 全部停止**：只操作「已启用」的实例。
- **新建**：三种实例的默认模板（带示例字段）。
- **打开 / 保存**：读写左侧路径指向的 JSON（默认 `~/.devicesim/profile.json`）。
- **导出**：把当前方案复制到剪贴板，便于换机器。
- **立即发送/轮询一次**：运行中的 MQTT 发布或 Modbus 客户端操作立刻执行一轮。

## 3. MQTT 客户端

必填：`Broker 主机`、`端口`。Client ID 留空会生成 `devicesim-<实例id>`。

常用项：

- **协议**：MQTTv31 / MQTTv311 / MQTTv5。
- **TLS**：勾选后可填 CA、客户端证书；「跳过证书校验」仅用于实验室自签证书。
- **Birth / LWT**：上线与异常掉线时自动发布状态，主题支持 `${name}` `${id}`。
- **发布规则**：周期、抖动、QoS、Retain；载荷为 JSON 对象、文本模板或十六进制。
- **订阅**：可收平台下发的命令，内容出现在日志里。

JSON 示例字段见 `examples/mqtt-temperature-sensor.json`。

## 4. Modbus TCP 客户端（主站）

连接到已有设备或本工具从站。

- **单元 ID**：对应 Slave ID。
- **操作**：每个操作独立周期。读操作用于监视；写操作按「写字段」规则生成再下发。
- **解码类型 / 字节序**：读 float32/int32 时需要与设备一致，默认 `ABCD`（大端）。

支持的功能：`read_coils`、`read_discrete_inputs`、`read_holding_registers`、`read_input_registers`、`write_coil`、`write_register`、`write_coils`、`write_registers`。

先启动从站再启动客户端。本机互相对打示例：同时加载 `examples/modbus-tcp-slave.json` 与 `examples/modbus-tcp-client.json`（或直接用 `mixed-gateway.json`）。

## 5. Modbus TCP 从站（设备）

绑定 `0.0.0.0` 或 `127.0.0.1` 与端口（不要用未授权的 502，开发时用 5020）。每个从站实例占用一个端口；复制实例时会自动 +1 端口。

寄存器映射的每一行：

- **数据区**：coils / discrete_inputs / holding_registers / input_registers
- **地址**：从 0 开始（内部使用 zero-based）
- **类型**：bool / uint16 / int16 / uint32 / int32 / float32（32 位占 2 个寄存器）
- **刷新周期**：按规则改写数据区，主站下一次读取就能拿到新值

## 6. 无界面模式

CI、服务器或长时间烧机可用：

```bash
npm run cli -- run examples/mixed-gateway.json --duration 60
```

`--duration 0`（默认）表示直到 Ctrl+C。日志打印到标准输出。

## 7. 方案文件协作

- 用 `npm run cli -- validate path.json` 检查字段。
- 用 `npm run cli -- schema` 生成 JSON Schema，可供 VS Code 关联。
- 方案可在 macOS 与 Windows 之间直接复制，路径分隔符只出现在 TLS 证书路径里，换系统时注意改一下。

## 8. 建议的实验室拓扑

1. 本机 Mosquitto 监听 1883。
2. DeviceSim 启动 3 个 MQTT 客户端（`mqtt-multi-devices.json`）。
3. 另开一个从站 5020 + 一个主站去轮询（或用第三方 Modbus Poll）。
4. 看日志确认发布与寄存器刷新；用 MQTTX / mosquitto_sub 抽查主题。
