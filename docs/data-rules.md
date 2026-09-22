# 数据生成规则

字段结构（MQTT JSON 字段与 Modbus 写/刷新字段共用）：

```json
{
  "name": "temperature",
  "type": "float",
  "mode": "walk",
  "min": 18,
  "max": 28,
  "start": 22,
  "step": 0.3,
  "decimals": 1
}
```

## 类型 `type`

| 值 | 说明 |
| --- | --- |
| `int` / `int16` / `uint16` / `int32` / `uint32` | 整数，超出类型范围会被截断 |
| `float` / `float32` | 浮点，`decimals` 控制小数位 |
| `bool` | 布尔；随机模式用 `probability` |
| `string` | 字符串 |
| `hex` | 随机十六进制，长度由 `hex_bytes` 决定 |

## 模式 `mode`

| 模式 | 行为 | 主要参数 |
| --- | --- | --- |
| `const` | 固定值 | `value` |
| `random` | 均匀随机 | `min` `max`；bool 用 `probability` |
| `gaussian` | 正态随机并夹紧到区间 | `mean` `std` `min` `max` |
| `increment` | 每次加 `step` | `start` `step` `max` `wrap` |
| `sine` | 正弦波 | `min` `max` `period`（秒） |
| `enum` | 离散取值 | `values`，可选 `weights` |
| `walk` | 随机游走（适合温度等） | `start` `step` `min` `max` |
| `timestamp` | 当前时间 | `template`=`iso8601` / `epoch` / `epoch_ms` / strftime |
| `uuid` | UUID4 | 无 |
| `template` | 字符串替换 | `template`，占位符见下 |

## 占位符

主题、文本载荷、模板字段支持：

- `${name}` 实例名
- `${id}` 实例 ID
- `${字段名}` 同一次生成中已算出的字段
- `{{name}}` 写法等价

MQTT `payload_kind`：

- `json`：按字段表生成对象后 `json.dumps`
- `text`：渲染 `text_template`
- `hex`：把模板当十六进制字节发送

## Modbus 编码

- `bool` → 线圈/离散量 0/1
- 16 位 → 1 个寄存器
- `int32` / `uint32` / `float32` → 2 个寄存器
- `byte_order`：`ABCD`（默认大端）、`CDAB`（字交换）、`BADC`、`DCBA`（小端）

## 周期与抖动

`interval_ms` 是基准周期。`jitter_ms` 为 ± 抖动，用来避免多设备齐步发射。首次调度也会带一点抖动。

## 示例：带权重的枚举 + 游走温度

```json
[
  {"name": "temperature", "type": "float", "mode": "walk", "min": 18, "max": 30, "start": 24, "step": 0.2, "decimals": 1},
  {"name": "status", "type": "string", "mode": "enum", "values": ["ok", "warn", "fault"], "weights": [0.9, 0.08, 0.02]}
]
```
