from __future__ import annotations

from enum import Enum

from nicegui import ui

from devicesim.core.models import (
    ByteOrder,
    DataField,
    FieldType,
    GenMode,
    ModbusFunction,
    ModbusOperation,
    ModbusRegisterDef,
    ModbusTable,
    MqttPublishRule,
    MqttSubscription,
    SimInstance,
    as_uint,
)

MODE_LABELS = {
    "const": "常量",
    "random": "均匀随机",
    "gaussian": "高斯随机",
    "increment": "递增",
    "sine": "正弦波",
    "enum": "枚举",
    "walk": "随机游走",
    "timestamp": "时间戳",
    "uuid": "UUID",
    "template": "模板",
}

TYPE_LABELS = {t.value: t.value for t in FieldType}
FN_LABELS = {f.value: f.value for f in ModbusFunction}


def section(title: str):
    ui.label(title).classes("text-sm uppercase tracking-wider text-teal-400 mt-4 mb-2")


def enum_label(value: object) -> str:
    """枚举或被界面写成的同值字符串，都显示成选项文本。"""
    if isinstance(value, Enum):
        return str(value.value)
    return str(value)


def format_const(value: object) -> str:
    if value is None:
        return ""
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


def parse_const(text: str | None, field_type: FieldType) -> object:
    """把常量输入框的文本写成字段实际使用的 value。"""
    raw = (text or "").strip()
    if raw == "":
        return None
    if field_type == FieldType.BOOL:
        return raw.lower() in {"1", "true", "yes", "on"}
    if field_type in {FieldType.STRING, FieldType.HEX}:
        return raw
    try:
        number = float(raw)
    except ValueError:
        return raw
    if field_type in {FieldType.FLOAT, FieldType.FLOAT32}:
        return number
    return int(round(number))


def field_editor(field: DataField, on_delete=None) -> None:
    with ui.card().classes("w-full bg-slate-900/70 border border-slate-700"):
        with ui.row().classes("w-full items-end gap-2 flex-wrap"):
            ui.input("字段名").bind_value(field, "name").classes("w-36")
            ui.select(TYPE_LABELS, label="类型").bind_value(field, "type").classes("w-28")
            ui.select(MODE_LABELS, label="生成模式").bind_value(field, "mode").classes("w-36")
            ui.number("最小值").bind_value(field, "min").classes("w-24")
            ui.number("最大值").bind_value(field, "max").classes("w-24")
            ui.number("步长/幅度").bind_value(field, "step").classes("w-24")
            ui.number("小数位", format="%.0f").bind_value(field, "decimals").classes("w-24")
            if on_delete:
                ui.button(icon="delete", on_click=on_delete).props("flat dense color=negative")
        with ui.row().classes("w-full items-end gap-2 flex-wrap"):
            const_box = ui.input("常量", value=format_const(field.value)).classes("w-40").props("placeholder=1 或 true")

            def _apply_const(e, f=field) -> None:
                f.value = parse_const(e.value, f.type)

            const_box.on_value_change(_apply_const)
            ui.input("模板").bind_value(field, "template").classes("w-64")
            ui.input("枚举值(逗号分隔)").classes("w-64").props("placeholder=ok,warn,error").on(
                "blur",
                lambda e, f=field: setattr(
                    f, "values", [x.strip() for x in (e.sender.value or "").split(",") if x.strip()]
                ),
            )
            ui.number("概率(bool)", min=0, max=1, step=0.05).bind_value(field, "probability").classes("w-28")
            ui.number("正弦周期(秒)").bind_value(field, "period").classes("w-28")
            ui.number("高斯均值").bind_value(field, "mean").classes("w-28")
            ui.number("高斯标准差").bind_value(field, "std").classes("w-28")


def render_mqtt(inst: SimInstance, on_change) -> None:
    cfg = inst.mqtt
    assert cfg is not None
    section("连接")
    with ui.row().classes("w-full gap-3 flex-wrap"):
        ui.input("Broker 主机").bind_value(cfg, "host").classes("w-48")
        ui.number("端口", format="%.0f").bind_value(cfg, "port").classes("w-28")
        ui.input("Client ID（空则自动）").bind_value(cfg, "client_id").classes("w-56")
        ui.select(["MQTTv31", "MQTTv311", "MQTTv5"], label="协议").bind_value(cfg, "protocol").classes("w-36")
        ui.number("Keepalive", format="%.0f").bind_value(cfg, "keepalive").classes("w-28")
    with ui.row().classes("w-full gap-3 flex-wrap items-center"):
        ui.input("用户名").bind_value(cfg, "username").classes("w-40")
        ui.input("密码").bind_value(cfg, "password").props("type=password").classes("w-40")
        ui.switch("Clean Session").bind_value(cfg, "clean_session")
        ui.switch("TLS").bind_value(cfg, "use_tls")
        ui.switch("跳过证书校验").bind_value(cfg, "tls_insecure")
        ui.switch("自动重连").bind_value(cfg, "auto_reconnect")
    with ui.row().classes("w-full gap-3 flex-wrap"):
        ui.input("CA 证书").bind_value(cfg, "ca_certs").classes("w-64")
        ui.input("客户端证书").bind_value(cfg, "certfile").classes("w-64")
        ui.input("客户端密钥").bind_value(cfg, "keyfile").classes("w-64")
    section("上线 / 遗嘱")
    with ui.row().classes("w-full gap-3 flex-wrap"):
        ui.input("Birth Topic").bind_value(cfg, "birth_topic").classes("w-72")
        ui.input("Birth Payload").bind_value(cfg, "birth_payload").classes("w-72")
        ui.input("LWT Topic").bind_value(cfg, "lwt_topic").classes("w-72")
        ui.input("LWT Payload").bind_value(cfg, "lwt_payload").classes("w-72")

    section("发布规则")
    ui.label("主题支持 ${name} ${id} 以及字段名占位符。").classes("text-xs text-slate-400")

    def add_pub():
        cfg.publishes.append(
            MqttPublishRule(
                name=f"pub-{len(cfg.publishes)+1}",
                fields=[DataField(name="value", type=FieldType.FLOAT, min=0, max=100)],
            )
        )
        on_change()

    ui.button("添加发布规则", icon="add", on_click=add_pub).props("outline dense")
    for i, rule in enumerate(list(cfg.publishes)):
        with ui.expansion(f"{rule.name}  →  {rule.topic}", icon="publish").classes("w-full"):
            with ui.row().classes("w-full gap-2 flex-wrap items-end"):
                ui.switch("启用").bind_value(rule, "enabled")
                ui.input("名称").bind_value(rule, "name").classes("w-36")
                ui.input("Topic").bind_value(rule, "topic").classes("w-80")
                ui.select({0: "QoS 0", 1: "QoS 1", 2: "QoS 2"}, label="QoS").bind_value(rule, "qos").classes("w-28")
                ui.switch("Retain").bind_value(rule, "retain")
                ui.number("周期(ms)", format="%.0f").bind_value(rule, "interval_ms").classes("w-28")
                ui.number("抖动(ms)", format="%.0f").bind_value(rule, "jitter_ms").classes("w-28")
                ui.select(
                    {"json": "JSON 对象", "text": "文本模板", "hex": "十六进制"},
                    label="载荷",
                ).bind_value(rule, "payload_kind").classes("w-36")
                ui.button(
                    icon="delete",
                    on_click=lambda r=rule: (cfg.publishes.remove(r), on_change()),
                ).props("flat dense color=negative")
            ui.textarea("文本/Hex 模板").bind_value(rule, "text_template").classes("w-full")
            ui.label("JSON 字段").classes("text-xs text-slate-400")
            for field in list(rule.fields):
                field_editor(field, on_delete=lambda f=field, r=rule: (r.fields.remove(f), on_change()))
            ui.button(
                "添加字段",
                icon="add",
                on_click=lambda r=rule: (r.fields.append(DataField(min=0, max=100)), on_change()),
            ).props("flat dense")

    section("订阅")

    def add_sub():
        cfg.subscriptions.append(MqttSubscription())
        on_change()

    ui.button("添加订阅", icon="add", on_click=add_sub).props("outline dense")
    for sub in list(cfg.subscriptions):
        with ui.row().classes("w-full items-end gap-2"):
            ui.switch("启用").bind_value(sub, "enabled")
            ui.input("Topic Filter").bind_value(sub, "topic").classes("w-96")
            ui.select({0: "QoS 0", 1: "QoS 1", 2: "QoS 2"}, label="QoS").bind_value(sub, "qos").classes("w-28")
            ui.button(icon="delete", on_click=lambda s=sub: (cfg.subscriptions.remove(s), on_change())).props(
                "flat dense color=negative"
            )


def render_modbus_client(inst: SimInstance, on_change) -> None:
    cfg = inst.modbus_client
    assert cfg is not None
    section("连接")
    with ui.row().classes("w-full gap-3 flex-wrap items-center"):
        ui.input("主机").bind_value(cfg, "host").classes("w-48")
        ui.number("端口", format="%.0f").bind_value(cfg, "port").classes("w-28")
        ui.number("单元 ID / Slave", format="%.0f").bind_value(cfg, "unit_id").classes("w-36")
        ui.number("超时(秒)").bind_value(cfg, "timeout").classes("w-28")
        ui.select({b.value: b.value for b in ByteOrder}, label="字节序").bind_value(cfg, "byte_order").classes("w-28")
        ui.switch("自动重连").bind_value(cfg, "auto_reconnect")

    section("周期操作")
    ui.label("读操作用于轮询设备；写操作按字段规则生成数值。").classes("text-xs text-slate-400")

    def add_op():
        cfg.operations.append(ModbusOperation(name=f"op-{len(cfg.operations)+1}"))
        on_change()

    ui.button("添加操作", icon="add", on_click=add_op).props("outline dense")
    for op in list(cfg.operations):
        with ui.expansion(f"{op.name}  {enum_label(op.function)} @{op.address}", icon="sync").classes("w-full"):
            with ui.row().classes("w-full gap-2 flex-wrap items-end"):
                ui.switch("启用").bind_value(op, "enabled")
                ui.input("名称").bind_value(op, "name").classes("w-36")
                ui.select(FN_LABELS, label="功能码").bind_value(op, "function").classes("w-64")
                ui.number("地址", format="%.0f").bind_value(op, "address").classes("w-24")
                ui.number("数量", format="%.0f").bind_value(op, "count").classes("w-24")
                ui.number("周期(ms)", format="%.0f").bind_value(op, "interval_ms").classes("w-28")
                ui.number("抖动(ms)", format="%.0f").bind_value(op, "jitter_ms").classes("w-28")
                ui.select(TYPE_LABELS, label="解码类型").bind_value(op, "data_type").classes("w-32")
                ui.select({b.value: b.value for b in ByteOrder}, label="字节序").bind_value(op, "byte_order").classes(
                    "w-28"
                )
                ui.button(icon="delete", on_click=lambda o=op: (cfg.operations.remove(o), on_change())).props(
                    "flat dense color=negative"
                )
            ui.label("写操作字段").classes("text-xs text-slate-400")
            for field in list(op.fields):
                field_editor(field, on_delete=lambda f=field, o=op: (o.fields.remove(f), on_change()))
            ui.button(
                "添加写字段",
                icon="add",
                on_click=lambda o=op: (o.fields.append(DataField(name="value", type=FieldType.UINT16, decimals=0)), on_change()),
            ).props("flat dense")


def render_modbus_slave(inst: SimInstance, on_change) -> None:
    cfg = inst.modbus_slave
    assert cfg is not None
    section("监听")
    with ui.row().classes("w-full gap-3 flex-wrap"):
        ui.input("绑定地址").bind_value(cfg, "host").classes("w-40")
        ui.number("端口", format="%.0f").bind_value(cfg, "port").classes("w-28")
        ui.number("单元 ID", format="%.0f").bind_value(cfg, "unit_id").classes("w-28")
        ui.input("厂商").bind_value(cfg, "identity_vendor").classes("w-40")
        ui.input("产品名").bind_value(cfg, "identity_product").classes("w-48")
    with ui.row().classes("w-full gap-3 flex-wrap"):
        ui.number("Coils 长度", format="%.0f").bind_value(cfg, "coils_size").classes("w-32")
        ui.number("离散量长度", format="%.0f").bind_value(cfg, "discrete_inputs_size").classes("w-32")
        ui.number("保持寄存器长度", format="%.0f").bind_value(cfg, "holding_registers_size").classes("w-36")
        ui.number("输入寄存器长度", format="%.0f").bind_value(cfg, "input_registers_size").classes("w-36")

    section("寄存器映射 / 数据生成")
    ui.label("仿真设备侧：按规则刷新数据区，供真实或虚拟主站读取。").classes("text-xs text-slate-400")

    def add_reg():
        cfg.registers.append(ModbusRegisterDef(name=f"reg-{len(cfg.registers)+1}"))
        on_change()

    ui.button("添加寄存器", icon="add", on_click=add_reg).props("outline dense")
    table_labels = {t.value: t.value for t in ModbusTable}
    for reg in list(cfg.registers):
        with ui.expansion(f"{reg.name}  {enum_label(reg.table)}[{reg.address}]", icon="memory").classes("w-full"):
            with ui.row().classes("w-full gap-2 flex-wrap items-end"):
                ui.switch("启用").bind_value(reg, "enabled")
                ui.input("名称").bind_value(reg, "name").classes("w-36")
                ui.select(table_labels, label="数据区").bind_value(reg, "table").classes("w-52")
                ui.number("地址", format="%.0f", min=0, precision=0).bind_value(
                    reg, "address", forward=as_uint
                ).classes("w-24")
                ui.select(TYPE_LABELS, label="类型").bind_value(reg, "data_type").classes("w-32")
                ui.select({b.value: b.value for b in ByteOrder}, label="字节序").bind_value(reg, "byte_order").classes(
                    "w-28"
                )
                ui.number("刷新(ms)", format="%.0f").bind_value(reg, "interval_ms").classes("w-28")
                ui.number("抖动(ms)", format="%.0f").bind_value(reg, "jitter_ms").classes("w-28")
                ui.button(icon="delete", on_click=lambda r=reg: (cfg.registers.remove(r), on_change())).props(
                    "flat dense color=negative"
                )
            field_editor(reg.field)
