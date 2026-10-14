from __future__ import annotations

import uuid
from enum import Enum
from typing import Any, Literal

from pydantic import BaseModel, Field, field_validator, model_validator


def new_id() -> str:
    return uuid.uuid4().hex[:12]


def as_uint(value: Any, *, maximum: int = 65535) -> int:
    """把界面数字框写入的 float 或空值收成无符号整数。"""
    if value is None or value == "":
        return 0
    number = int(value)
    if number < 0:
        return 0
    if number > maximum:
        return maximum
    return number


class SimModel(BaseModel):
    """配置模型。下拉框双向绑定会把枚举写成字符串，这里在赋值时收回到枚举。"""

    def __setattr__(self, name: str, value: Any) -> None:
        if isinstance(value, str):
            field = self.__class__.model_fields.get(name)
            annotation = field.annotation if field is not None else None
            if isinstance(annotation, type) and issubclass(annotation, Enum):
                try:
                    value = annotation(value)
                except ValueError:
                    pass
        super().__setattr__(name, value)


def _uint16_field() -> DataField:
    return DataField(
        name="value",
        type=FieldType.UINT16,
        mode=GenMode.RANDOM,
        min=0,
        max=65535,
        start=0,
        decimals=0,
        value=0,
    )


class InstanceType(str, Enum):
    MQTT_CLIENT = "mqtt_client"
    MODBUS_TCP_CLIENT = "modbus_tcp_client"
    MODBUS_TCP_SLAVE = "modbus_tcp_slave"


class FieldType(str, Enum):
    INT = "int"
    FLOAT = "float"
    BOOL = "bool"
    STRING = "string"
    INT16 = "int16"
    UINT16 = "uint16"
    INT32 = "int32"
    UINT32 = "uint32"
    FLOAT32 = "float32"
    HEX = "hex"


class GenMode(str, Enum):
    CONST = "const"
    RANDOM = "random"
    GAUSSIAN = "gaussian"
    INCREMENT = "increment"
    SINE = "sine"
    ENUM = "enum"
    WALK = "walk"
    TIMESTAMP = "timestamp"
    UUID = "uuid"
    TEMPLATE = "template"


class ByteOrder(str, Enum):
    ABCD = "ABCD"
    CDAB = "CDAB"
    BADC = "BADC"
    DCBA = "DCBA"


class DataField(SimModel):
    """单字段生成规则。MQTT JSON 与 Modbus 寄存器共用。"""

    name: str = "value"
    type: FieldType = FieldType.FLOAT
    mode: GenMode = GenMode.RANDOM
    value: Any = None
    min: float | None = None
    max: float | None = None
    mean: float | None = None
    std: float | None = None
    start: float = 0
    step: float = 1
    period: float = 60
    decimals: int | None = 2
    values: list[Any] = Field(default_factory=list)
    weights: list[float] = Field(default_factory=list)
    template: str = ""
    probability: float = 0.5
    wrap: bool = True
    hex_bytes: int = 4

    model_config = {"extra": "ignore"}


class MqttPublishRule(SimModel):
    enabled: bool = True
    name: str = "telemetry"
    topic: str = "devices/${name}/telemetry"
    qos: int = 0
    retain: bool = False
    interval_ms: int = 1000
    jitter_ms: int = 0
    payload_kind: Literal["json", "text", "hex"] = "json"
    text_template: str = '{"value": ${value}}'
    fields: list[DataField] = Field(default_factory=list)


class MqttSubscription(SimModel):
    enabled: bool = True
    topic: str = "devices/${name}/cmd/#"
    qos: int = 0


class MqttClientConfig(SimModel):
    host: str = "127.0.0.1"
    port: int = 1883
    client_id: str = ""
    username: str = ""
    password: str = ""
    keepalive: int = 60
    protocol: Literal["MQTTv31", "MQTTv311", "MQTTv5"] = "MQTTv311"
    clean_session: bool = True
    use_tls: bool = False
    tls_insecure: bool = False
    ca_certs: str = ""
    certfile: str = ""
    keyfile: str = ""
    lwt_topic: str = ""
    lwt_payload: str = '{"online": false}'
    lwt_qos: int = 0
    lwt_retain: bool = True
    birth_topic: str = ""
    birth_payload: str = '{"online": true}'
    birth_qos: int = 0
    birth_retain: bool = True
    auto_reconnect: bool = True
    publishes: list[MqttPublishRule] = Field(default_factory=list)
    subscriptions: list[MqttSubscription] = Field(default_factory=list)


class ModbusFunction(str, Enum):
    READ_COILS = "read_coils"
    READ_DISCRETE_INPUTS = "read_discrete_inputs"
    READ_HOLDING_REGISTERS = "read_holding_registers"
    READ_INPUT_REGISTERS = "read_input_registers"
    WRITE_COIL = "write_coil"
    WRITE_REGISTER = "write_register"
    WRITE_COILS = "write_coils"
    WRITE_REGISTERS = "write_registers"


class ModbusOperation(SimModel):
    enabled: bool = True
    name: str = "poll"
    function: ModbusFunction = ModbusFunction.READ_HOLDING_REGISTERS
    address: int = 0
    count: int = 10
    interval_ms: int = 1000
    jitter_ms: int = 0
    data_type: FieldType = FieldType.UINT16
    byte_order: ByteOrder = ByteOrder.ABCD
    fields: list[DataField] = Field(default_factory=list)


class ModbusTcpClientConfig(SimModel):
    host: str = "127.0.0.1"
    port: int = 5020
    unit_id: int = 1
    timeout: float = 3.0
    auto_reconnect: bool = True
    byte_order: ByteOrder = ByteOrder.ABCD
    operations: list[ModbusOperation] = Field(default_factory=list)


class ModbusTable(str, Enum):
    COILS = "coils"
    DISCRETE_INPUTS = "discrete_inputs"
    HOLDING_REGISTERS = "holding_registers"
    INPUT_REGISTERS = "input_registers"


class ModbusRegisterDef(SimModel):
    enabled: bool = True
    name: str = "reg"
    table: ModbusTable = ModbusTable.HOLDING_REGISTERS
    address: int = 0
    data_type: FieldType = FieldType.UINT16
    byte_order: ByteOrder = ByteOrder.ABCD
    interval_ms: int = 1000
    jitter_ms: int = 0
    field: DataField = Field(default_factory=_uint16_field)

    @field_validator("address", mode="before")
    @classmethod
    def _address_uint(cls, value: Any) -> int:
        return as_uint(value)


class ModbusTcpSlaveConfig(SimModel):
    host: str = "0.0.0.0"
    port: int = 5020
    unit_id: int = 1
    coils_size: int = 2000
    discrete_inputs_size: int = 2000
    holding_registers_size: int = 2000
    input_registers_size: int = 2000
    identity_vendor: str = "DeviceSim"
    identity_product: str = "Virtual Device"
    identity_version: str = "1.0.0"
    registers: list[ModbusRegisterDef] = Field(default_factory=list)


class SimInstance(SimModel):
    id: str = Field(default_factory=new_id)
    name: str = "device-1"
    type: InstanceType = InstanceType.MQTT_CLIENT
    enabled: bool = True
    note: str = ""
    mqtt: MqttClientConfig | None = None
    modbus_client: ModbusTcpClientConfig | None = None
    modbus_slave: ModbusTcpSlaveConfig | None = None

    @model_validator(mode="after")
    def _ensure_config(self) -> SimInstance:
        if self.type == InstanceType.MQTT_CLIENT and self.mqtt is None:
            self.mqtt = MqttClientConfig()
        if self.type == InstanceType.MODBUS_TCP_CLIENT and self.modbus_client is None:
            self.modbus_client = ModbusTcpClientConfig()
        if self.type == InstanceType.MODBUS_TCP_SLAVE and self.modbus_slave is None:
            self.modbus_slave = ModbusTcpSlaveConfig()
        return self


class Profile(SimModel):
    version: int = 1
    name: str = "未命名方案"
    description: str = ""
    instances: list[SimInstance] = Field(default_factory=list)


def default_mqtt_instance(name: str = "mqtt-sensor-1") -> SimInstance:
    return SimInstance(
        name=name,
        type=InstanceType.MQTT_CLIENT,
        mqtt=MqttClientConfig(
            publishes=[
                MqttPublishRule(
                    name="telemetry",
                    topic="devices/${name}/telemetry",
                    interval_ms=1000,
                    fields=[
                        DataField(
                            name="temperature",
                            type=FieldType.FLOAT,
                            mode=GenMode.WALK,
                            min=18,
                            max=28,
                            start=22,
                            step=0.3,
                            decimals=1,
                        ),
                        DataField(
                            name="humidity",
                            type=FieldType.FLOAT,
                            mode=GenMode.RANDOM,
                            min=40,
                            max=70,
                            decimals=1,
                        ),
                        DataField(name="ts", type=FieldType.STRING, mode=GenMode.TIMESTAMP),
                    ],
                )
            ],
            subscriptions=[MqttSubscription(topic="devices/${name}/cmd/#")],
            birth_topic="devices/${name}/status",
            lwt_topic="devices/${name}/status",
        ),
    )


def default_modbus_client_instance(name: str = "mb-master-1") -> SimInstance:
    return SimInstance(
        name=name,
        type=InstanceType.MODBUS_TCP_CLIENT,
        modbus_client=ModbusTcpClientConfig(
            operations=[
                ModbusOperation(
                    name="read-holding",
                    function=ModbusFunction.READ_HOLDING_REGISTERS,
                    address=0,
                    count=10,
                    interval_ms=1000,
                ),
                ModbusOperation(
                    name="write-setpoint",
                    function=ModbusFunction.WRITE_REGISTER,
                    address=100,
                    count=1,
                    interval_ms=2000,
                    fields=[
                        DataField(
                            name="setpoint",
                            type=FieldType.UINT16,
                            mode=GenMode.RANDOM,
                            min=0,
                            max=1000,
                            decimals=0,
                        )
                    ],
                ),
            ]
        ),
    )


def default_modbus_slave_instance(name: str = "mb-device-1", port: int = 5020) -> SimInstance:
    return SimInstance(
        name=name,
        type=InstanceType.MODBUS_TCP_SLAVE,
        modbus_slave=ModbusTcpSlaveConfig(
            port=port,
            registers=[
                ModbusRegisterDef(
                    name="temperature_x10",
                    table=ModbusTable.HOLDING_REGISTERS,
                    address=0,
                    data_type=FieldType.INT16,
                    interval_ms=500,
                    field=DataField(
                        name="temperature_x10",
                        type=FieldType.INT16,
                        mode=GenMode.WALK,
                        min=180,
                        max=280,
                        start=220,
                        step=2,
                        decimals=0,
                    ),
                ),
                ModbusRegisterDef(
                    name="humidity_x10",
                    table=ModbusTable.HOLDING_REGISTERS,
                    address=1,
                    data_type=FieldType.UINT16,
                    interval_ms=500,
                    field=DataField(
                        name="humidity_x10",
                        type=FieldType.UINT16,
                        mode=GenMode.RANDOM,
                        min=400,
                        max=700,
                        decimals=0,
                    ),
                ),
                ModbusRegisterDef(
                    name="running",
                    table=ModbusTable.COILS,
                    address=0,
                    data_type=FieldType.BOOL,
                    interval_ms=2000,
                    field=DataField(
                        name="running",
                        type=FieldType.BOOL,
                        mode=GenMode.ENUM,
                        values=[True],
                    ),
                ),
                ModbusRegisterDef(
                    name="alarm",
                    table=ModbusTable.DISCRETE_INPUTS,
                    address=0,
                    data_type=FieldType.BOOL,
                    interval_ms=3000,
                    field=DataField(
                        name="alarm",
                        type=FieldType.BOOL,
                        mode=GenMode.RANDOM,
                        probability=0.05,
                    ),
                ),
            ],
        ),
    )


def default_profile() -> Profile:
    return Profile(
        name="示例：温湿度设备",
        description="一个 MQTT 传感器、一个 Modbus 从站、一个 Modbus 主站轮询示例。",
        instances=[
            default_mqtt_instance(),
            default_modbus_slave_instance(),
            default_modbus_client_instance(),
        ],
    )


TYPE_LABELS = {
    InstanceType.MQTT_CLIENT: "MQTT 客户端",
    InstanceType.MODBUS_TCP_CLIENT: "Modbus TCP 客户端",
    InstanceType.MODBUS_TCP_SLAVE: "Modbus TCP 从站",
}
