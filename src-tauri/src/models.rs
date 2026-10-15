use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub fn new_id() -> String {
    Uuid::new_v4().simple().to_string()[..12].to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstanceType {
    MqttClient,
    ModbusTcpClient,
    ModbusTcpSlave,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Int,
    #[default]
    Float,
    Bool,
    String,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float32,
    Hex,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GenMode {
    Const,
    #[default]
    Random,
    Gaussian,
    Increment,
    Sine,
    Enum,
    Walk,
    Timestamp,
    Uuid,
    Template,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub enum ByteOrder {
    #[default]
    ABCD,
    CDAB,
    BADC,
    DCBA,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DataField {
    #[serde(default = "default_field_name")]
    pub name: String,
    #[serde(default, rename = "type")]
    pub field_type: FieldType,
    #[serde(default)]
    pub mode: GenMode,
    #[serde(default)]
    pub value: Option<Value>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub mean: Option<f64>,
    #[serde(default)]
    pub std: Option<f64>,
    #[serde(default)]
    pub start: f64,
    #[serde(default = "default_step")]
    pub step: f64,
    #[serde(default = "default_period")]
    pub period: f64,
    #[serde(default = "default_decimals")]
    pub decimals: Option<i32>,
    #[serde(default)]
    pub values: Vec<Value>,
    #[serde(default)]
    pub weights: Vec<f64>,
    #[serde(default)]
    pub template: String,
    #[serde(default = "default_probability")]
    pub probability: f64,
    #[serde(default = "default_true")]
    pub wrap: bool,
    #[serde(default = "default_hex_bytes")]
    pub hex_bytes: usize,
}

fn default_field_name() -> String {
    "value".into()
}
fn default_step() -> f64 {
    1.0
}
fn default_period() -> f64 {
    60.0
}
fn default_decimals() -> Option<i32> {
    Some(2)
}
fn default_probability() -> f64 {
    0.5
}
fn default_true() -> bool {
    true
}
fn default_hex_bytes() -> usize {
    4
}

impl Default for DataField {
    fn default() -> Self {
        Self {
            name: default_field_name(),
            field_type: FieldType::Float,
            mode: GenMode::Random,
            value: None,
            min: None,
            max: None,
            mean: None,
            std: None,
            start: 0.0,
            step: 1.0,
            period: 60.0,
            decimals: Some(2),
            values: vec![],
            weights: vec![],
            template: String::new(),
            probability: 0.5,
            wrap: true,
            hex_bytes: 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MqttPublishRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_telemetry")]
    pub name: String,
    #[serde(default = "default_mqtt_topic")]
    pub topic: String,
    #[serde(default)]
    pub qos: u8,
    #[serde(default)]
    pub retain: bool,
    #[serde(default = "default_interval")]
    pub interval_ms: u64,
    #[serde(default)]
    pub jitter_ms: i64,
    #[serde(default = "default_payload_kind")]
    pub payload_kind: String,
    #[serde(default = "default_text_template")]
    pub text_template: String,
    #[serde(default)]
    pub fields: Vec<DataField>,
}

fn default_telemetry() -> String {
    "telemetry".into()
}
fn default_mqtt_topic() -> String {
    "devices/${name}/telemetry".into()
}
fn default_interval() -> u64 {
    1000
}
fn default_payload_kind() -> String {
    "json".into()
}
fn default_text_template() -> String {
    r#"{"value": ${value}}"#.into()
}

impl Default for MqttPublishRule {
    fn default() -> Self {
        Self {
            enabled: true,
            name: default_telemetry(),
            topic: default_mqtt_topic(),
            qos: 0,
            retain: false,
            interval_ms: 1000,
            jitter_ms: 0,
            payload_kind: "json".into(),
            text_template: default_text_template(),
            fields: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MqttSubscription {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_sub_topic")]
    pub topic: String,
    #[serde(default)]
    pub qos: u8,
}

fn default_sub_topic() -> String {
    "devices/${name}/cmd/#".into()
}

impl Default for MqttSubscription {
    fn default() -> Self {
        Self {
            enabled: true,
            topic: default_sub_topic(),
            qos: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MqttClientConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_mqtt_port")]
    pub port: u16,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_keepalive")]
    pub keepalive: u64,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default = "default_true")]
    pub clean_session: bool,
    #[serde(default)]
    pub use_tls: bool,
    #[serde(default)]
    pub tls_insecure: bool,
    #[serde(default)]
    pub ca_certs: String,
    #[serde(default)]
    pub certfile: String,
    #[serde(default)]
    pub keyfile: String,
    #[serde(default)]
    pub lwt_topic: String,
    #[serde(default = "default_lwt_payload")]
    pub lwt_payload: String,
    #[serde(default)]
    pub lwt_qos: u8,
    #[serde(default = "default_true")]
    pub lwt_retain: bool,
    #[serde(default)]
    pub birth_topic: String,
    #[serde(default = "default_birth_payload")]
    pub birth_payload: String,
    #[serde(default)]
    pub birth_qos: u8,
    #[serde(default = "default_true")]
    pub birth_retain: bool,
    #[serde(default = "default_true")]
    pub auto_reconnect: bool,
    #[serde(default)]
    pub publishes: Vec<MqttPublishRule>,
    #[serde(default)]
    pub subscriptions: Vec<MqttSubscription>,
}

fn default_host() -> String {
    "127.0.0.1".into()
}
fn default_mqtt_port() -> u16 {
    1883
}
fn default_keepalive() -> u64 {
    60
}
fn default_protocol() -> String {
    "MQTTv311".into()
}
fn default_lwt_payload() -> String {
    r#"{"online": false}"#.into()
}
fn default_birth_payload() -> String {
    r#"{"online": true}"#.into()
}

impl Default for MqttClientConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: 1883,
            client_id: String::new(),
            username: String::new(),
            password: String::new(),
            keepalive: 60,
            protocol: default_protocol(),
            clean_session: true,
            use_tls: false,
            tls_insecure: false,
            ca_certs: String::new(),
            certfile: String::new(),
            keyfile: String::new(),
            lwt_topic: String::new(),
            lwt_payload: default_lwt_payload(),
            lwt_qos: 0,
            lwt_retain: true,
            birth_topic: String::new(),
            birth_payload: default_birth_payload(),
            birth_qos: 0,
            birth_retain: true,
            auto_reconnect: true,
            publishes: vec![],
            subscriptions: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModbusFunction {
    ReadCoils,
    ReadDiscreteInputs,
    #[default]
    ReadHoldingRegisters,
    ReadInputRegisters,
    WriteCoil,
    WriteRegister,
    WriteCoils,
    WriteRegisters,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModbusOperation {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_poll")]
    pub name: String,
    #[serde(default)]
    pub function: ModbusFunction,
    #[serde(default)]
    pub address: u16,
    #[serde(default = "default_count")]
    pub count: u16,
    #[serde(default = "default_interval")]
    pub interval_ms: u64,
    #[serde(default)]
    pub jitter_ms: i64,
    #[serde(default = "default_uint16")]
    pub data_type: FieldType,
    #[serde(default)]
    pub byte_order: ByteOrder,
    #[serde(default)]
    pub fields: Vec<DataField>,
}

fn default_poll() -> String {
    "poll".into()
}
fn default_count() -> u16 {
    10
}
fn default_uint16() -> FieldType {
    FieldType::Uint16
}

impl Default for ModbusOperation {
    fn default() -> Self {
        Self {
            enabled: true,
            name: default_poll(),
            function: ModbusFunction::ReadHoldingRegisters,
            address: 0,
            count: 10,
            interval_ms: 1000,
            jitter_ms: 0,
            data_type: FieldType::Uint16,
            byte_order: ByteOrder::ABCD,
            fields: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModbusTcpClientConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_modbus_port")]
    pub port: u16,
    #[serde(default = "default_unit")]
    pub unit_id: u8,
    #[serde(default = "default_timeout")]
    pub timeout: f64,
    #[serde(default = "default_true")]
    pub auto_reconnect: bool,
    #[serde(default)]
    pub byte_order: ByteOrder,
    #[serde(default)]
    pub operations: Vec<ModbusOperation>,
}

fn default_modbus_port() -> u16 {
    5020
}
fn default_unit() -> u8 {
    1
}
fn default_timeout() -> f64 {
    3.0
}

impl Default for ModbusTcpClientConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: 5020,
            unit_id: 1,
            timeout: 3.0,
            auto_reconnect: true,
            byte_order: ByteOrder::ABCD,
            operations: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModbusTable {
    Coils,
    DiscreteInputs,
    #[default]
    HoldingRegisters,
    InputRegisters,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModbusRegisterDef {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_reg")]
    pub name: String,
    #[serde(default)]
    pub table: ModbusTable,
    #[serde(default)]
    pub address: u16,
    #[serde(default = "default_uint16")]
    pub data_type: FieldType,
    #[serde(default)]
    pub byte_order: ByteOrder,
    #[serde(default = "default_interval")]
    pub interval_ms: u64,
    #[serde(default)]
    pub jitter_ms: i64,
    #[serde(default = "default_reg_field")]
    pub field: DataField,
}

fn default_reg() -> String {
    "reg".into()
}
fn default_reg_field() -> DataField {
    DataField {
        name: "value".into(),
        field_type: FieldType::Uint16,
        mode: GenMode::Random,
        min: Some(0.0),
        max: Some(65535.0),
        decimals: Some(0),
        ..Default::default()
    }
}

impl Default for ModbusRegisterDef {
    fn default() -> Self {
        Self {
            enabled: true,
            name: default_reg(),
            table: ModbusTable::HoldingRegisters,
            address: 0,
            data_type: FieldType::Uint16,
            byte_order: ByteOrder::ABCD,
            interval_ms: 1000,
            jitter_ms: 0,
            field: default_reg_field(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModbusTcpSlaveConfig {
    #[serde(default = "default_bind")]
    pub host: String,
    #[serde(default = "default_modbus_port")]
    pub port: u16,
    #[serde(default = "default_unit")]
    pub unit_id: u8,
    #[serde(default = "default_table_size")]
    pub coils_size: usize,
    #[serde(default = "default_table_size")]
    pub discrete_inputs_size: usize,
    #[serde(default = "default_table_size")]
    pub holding_registers_size: usize,
    #[serde(default = "default_table_size")]
    pub input_registers_size: usize,
    #[serde(default = "default_vendor")]
    pub identity_vendor: String,
    #[serde(default = "default_product")]
    pub identity_product: String,
    #[serde(default = "default_version")]
    pub identity_version: String,
    #[serde(default)]
    pub registers: Vec<ModbusRegisterDef>,
}

fn default_bind() -> String {
    "0.0.0.0".into()
}
fn default_table_size() -> usize {
    2000
}
fn default_vendor() -> String {
    "DeviceSim".into()
}
fn default_product() -> String {
    "Virtual Device".into()
}
fn default_version() -> String {
    "1.0.0".into()
}

impl Default for ModbusTcpSlaveConfig {
    fn default() -> Self {
        Self {
            host: default_bind(),
            port: 5020,
            unit_id: 1,
            coils_size: 2000,
            discrete_inputs_size: 2000,
            holding_registers_size: 2000,
            input_registers_size: 2000,
            identity_vendor: default_vendor(),
            identity_product: default_product(),
            identity_version: default_version(),
            registers: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SimInstance {
    #[serde(default = "new_id")]
    pub id: String,
    #[serde(default = "default_device_name")]
    pub name: String,
    #[serde(default = "default_instance_type", rename = "type")]
    pub instance_type: InstanceType,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub mqtt: Option<MqttClientConfig>,
    #[serde(default)]
    pub modbus_client: Option<ModbusTcpClientConfig>,
    #[serde(default)]
    pub modbus_slave: Option<ModbusTcpSlaveConfig>,
}

fn default_device_name() -> String {
    "device-1".into()
}
fn default_instance_type() -> InstanceType {
    InstanceType::MqttClient
}

impl Default for SimInstance {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: default_device_name(),
            instance_type: InstanceType::MqttClient,
            enabled: true,
            note: String::new(),
            mqtt: Some(MqttClientConfig::default()),
            modbus_client: None,
            modbus_slave: None,
        }
    }
}

impl SimInstance {
    pub fn ensure_config(&mut self) {
        match self.instance_type {
            InstanceType::MqttClient if self.mqtt.is_none() => {
                self.mqtt = Some(MqttClientConfig::default());
            }
            InstanceType::ModbusTcpClient if self.modbus_client.is_none() => {
                self.modbus_client = Some(ModbusTcpClientConfig::default());
            }
            InstanceType::ModbusTcpSlave if self.modbus_slave.is_none() => {
                self.modbus_slave = Some(ModbusTcpSlaveConfig::default());
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Profile {
    #[serde(default = "default_profile_version")]
    pub version: u32,
    #[serde(default = "default_profile_name")]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub instances: Vec<SimInstance>,
}

fn default_profile_version() -> u32 {
    1
}
fn default_profile_name() -> String {
    "未命名方案".into()
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            version: 1,
            name: default_profile_name(),
            description: String::new(),
            instances: vec![],
        }
    }
}

pub fn type_label(t: &InstanceType) -> &'static str {
    match t {
        InstanceType::MqttClient => "MQTT 客户端",
        InstanceType::ModbusTcpClient => "Modbus TCP 客户端",
        InstanceType::ModbusTcpSlave => "Modbus TCP 从站",
    }
}

pub fn default_mqtt_instance(name: &str) -> SimInstance {
    SimInstance {
        id: new_id(),
        name: name.into(),
        instance_type: InstanceType::MqttClient,
        enabled: true,
        note: String::new(),
        mqtt: Some(MqttClientConfig {
            birth_topic: "devices/${name}/status".into(),
            lwt_topic: "devices/${name}/status".into(),
            publishes: vec![MqttPublishRule {
                fields: vec![
                    DataField {
                        name: "temperature".into(),
                        field_type: FieldType::Float,
                        mode: GenMode::Walk,
                        min: Some(18.0),
                        max: Some(28.0),
                        start: 22.0,
                        step: 0.3,
                        decimals: Some(1),
                        ..Default::default()
                    },
                    DataField {
                        name: "humidity".into(),
                        field_type: FieldType::Float,
                        mode: GenMode::Random,
                        min: Some(40.0),
                        max: Some(70.0),
                        decimals: Some(1),
                        ..Default::default()
                    },
                    DataField {
                        name: "ts".into(),
                        field_type: FieldType::String,
                        mode: GenMode::Timestamp,
                        decimals: None,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            subscriptions: vec![MqttSubscription::default()],
            ..Default::default()
        }),
        modbus_client: None,
        modbus_slave: None,
    }
}

pub fn default_modbus_client_instance(name: &str) -> SimInstance {
    SimInstance {
        id: new_id(),
        name: name.into(),
        instance_type: InstanceType::ModbusTcpClient,
        enabled: true,
        note: String::new(),
        mqtt: None,
        modbus_client: Some(ModbusTcpClientConfig {
            operations: vec![
                ModbusOperation {
                    name: "read-holding".into(),
                    function: ModbusFunction::ReadHoldingRegisters,
                    address: 0,
                    count: 10,
                    interval_ms: 1000,
                    ..Default::default()
                },
                ModbusOperation {
                    name: "write-setpoint".into(),
                    function: ModbusFunction::WriteRegister,
                    address: 100,
                    count: 1,
                    interval_ms: 2000,
                    fields: vec![DataField {
                        name: "setpoint".into(),
                        field_type: FieldType::Uint16,
                        mode: GenMode::Random,
                        min: Some(0.0),
                        max: Some(1000.0),
                        decimals: Some(0),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        modbus_slave: None,
    }
}

pub fn default_modbus_slave_instance(name: &str, port: u16) -> SimInstance {
    SimInstance {
        id: new_id(),
        name: name.into(),
        instance_type: InstanceType::ModbusTcpSlave,
        enabled: true,
        note: String::new(),
        mqtt: None,
        modbus_client: None,
        modbus_slave: Some(ModbusTcpSlaveConfig {
            port,
            registers: vec![
                ModbusRegisterDef {
                    name: "temperature_x10".into(),
                    table: ModbusTable::HoldingRegisters,
                    address: 0,
                    data_type: FieldType::Int16,
                    interval_ms: 500,
                    field: DataField {
                        name: "temperature_x10".into(),
                        field_type: FieldType::Int16,
                        mode: GenMode::Walk,
                        min: Some(180.0),
                        max: Some(280.0),
                        start: 220.0,
                        step: 2.0,
                        decimals: Some(0),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ModbusRegisterDef {
                    name: "humidity_x10".into(),
                    table: ModbusTable::HoldingRegisters,
                    address: 1,
                    data_type: FieldType::Uint16,
                    interval_ms: 500,
                    field: DataField {
                        name: "humidity_x10".into(),
                        field_type: FieldType::Uint16,
                        mode: GenMode::Random,
                        min: Some(400.0),
                        max: Some(700.0),
                        decimals: Some(0),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ModbusRegisterDef {
                    name: "running".into(),
                    table: ModbusTable::Coils,
                    address: 0,
                    data_type: FieldType::Bool,
                    interval_ms: 2000,
                    field: DataField {
                        name: "running".into(),
                        field_type: FieldType::Bool,
                        mode: GenMode::Enum,
                        values: vec![Value::Bool(true)],
                        decimals: None,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ModbusRegisterDef {
                    name: "alarm".into(),
                    table: ModbusTable::DiscreteInputs,
                    address: 0,
                    data_type: FieldType::Bool,
                    interval_ms: 3000,
                    field: DataField {
                        name: "alarm".into(),
                        field_type: FieldType::Bool,
                        mode: GenMode::Random,
                        probability: 0.05,
                        decimals: None,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
    }
}

pub fn default_profile() -> Profile {
    Profile {
        version: 1,
        name: "示例：温湿度设备".into(),
        description: "一个 MQTT 传感器、一个 Modbus 从站、一个 Modbus 主站轮询示例。".into(),
        instances: vec![
            default_mqtt_instance("mqtt-sensor-1"),
            default_modbus_slave_instance("mb-device-1", 5020),
            default_modbus_client_instance("mb-master-1"),
        ],
    }
}
