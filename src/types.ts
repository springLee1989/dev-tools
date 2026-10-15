export type InstanceType = "mqtt_client" | "modbus_tcp_client" | "modbus_tcp_slave";

export type FieldType =
  | "int"
  | "float"
  | "bool"
  | "string"
  | "int16"
  | "uint16"
  | "int32"
  | "uint32"
  | "float32"
  | "hex";

export type GenMode =
  | "const"
  | "random"
  | "gaussian"
  | "increment"
  | "sine"
  | "enum"
  | "walk"
  | "timestamp"
  | "uuid"
  | "template";

export type ByteOrder = "ABCD" | "CDAB" | "BADC" | "DCBA";

export interface DataField {
  name: string;
  type: FieldType;
  mode: GenMode;
  value?: unknown;
  min?: number | null;
  max?: number | null;
  mean?: number | null;
  std?: number | null;
  start?: number;
  step?: number;
  period?: number;
  decimals?: number | null;
  values?: unknown[];
  weights?: number[];
  template?: string;
  probability?: number;
  wrap?: boolean;
  hex_bytes?: number;
}

export interface MqttPublishRule {
  enabled: boolean;
  name: string;
  topic: string;
  qos: number;
  retain: boolean;
  interval_ms: number;
  jitter_ms: number;
  payload_kind: string;
  text_template: string;
  fields: DataField[];
}

export interface MqttSubscription {
  enabled: boolean;
  topic: string;
  qos: number;
}

export interface MqttClientConfig {
  host: string;
  port: number;
  client_id: string;
  username: string;
  password: string;
  keepalive: number;
  protocol: string;
  clean_session: boolean;
  use_tls: boolean;
  tls_insecure: boolean;
  ca_certs: string;
  certfile: string;
  keyfile: string;
  lwt_topic: string;
  lwt_payload: string;
  lwt_qos: number;
  lwt_retain: boolean;
  birth_topic: string;
  birth_payload: string;
  birth_qos: number;
  birth_retain: boolean;
  auto_reconnect: boolean;
  publishes: MqttPublishRule[];
  subscriptions: MqttSubscription[];
}

export type ModbusFunction =
  | "read_coils"
  | "read_discrete_inputs"
  | "read_holding_registers"
  | "read_input_registers"
  | "write_coil"
  | "write_register"
  | "write_coils"
  | "write_registers";

export interface ModbusOperation {
  enabled: boolean;
  name: string;
  function: ModbusFunction;
  address: number;
  count: number;
  interval_ms: number;
  jitter_ms: number;
  data_type: FieldType;
  byte_order: ByteOrder;
  fields: DataField[];
}

export interface ModbusTcpClientConfig {
  host: string;
  port: number;
  unit_id: number;
  timeout: number;
  auto_reconnect: boolean;
  byte_order: ByteOrder;
  operations: ModbusOperation[];
}

export type ModbusTable =
  | "coils"
  | "discrete_inputs"
  | "holding_registers"
  | "input_registers";

export interface ModbusRegisterDef {
  enabled: boolean;
  name: string;
  table: ModbusTable;
  address: number;
  data_type: FieldType;
  byte_order: ByteOrder;
  interval_ms: number;
  jitter_ms: number;
  field: DataField;
}

export interface ModbusTcpSlaveConfig {
  host: string;
  port: number;
  unit_id: number;
  /** 协议地址 = 寄存器地址 − 偏移量。缺省按 0 处理。 */
  address_offset?: number;
  /** 缺省为 true。为 false 时该表不套用地址偏移。 */
  coils_use_offset?: boolean;
  discrete_inputs_use_offset?: boolean;
  holding_registers_use_offset?: boolean;
  input_registers_use_offset?: boolean;
  coils_size: number;
  discrete_inputs_size: number;
  holding_registers_size: number;
  input_registers_size: number;
  identity_vendor: string;
  identity_product: string;
  identity_version: string;
  registers: ModbusRegisterDef[];
}

export interface SimInstance {
  id: string;
  name: string;
  type: InstanceType;
  enabled: boolean;
  note: string;
  mqtt?: MqttClientConfig | null;
  modbus_client?: ModbusTcpClientConfig | null;
  modbus_slave?: ModbusTcpSlaveConfig | null;
}

export interface Profile {
  version: number;
  name: string;
  description: string;
  instances: SimInstance[];
}

export interface InstanceStats {
  sent: number;
  received: number;
  errors: number;
  connected: boolean;
  last_error: string;
  started_at?: number | null;
  last_activity?: number | null;
}

export interface AppStateView {
  profile: Profile;
  profile_path?: string | null;
  running: Record<string, boolean>;
  stats: Record<string, InstanceStats>;
}

export interface SimEvent {
  ts: number;
  instance_id: string;
  instance_name: string;
  level: string;
  kind: string;
  message: string;
  payload?: unknown;
}

export const TYPE_LABELS: Record<InstanceType, string> = {
  mqtt_client: "MQTT 客户端",
  modbus_tcp_client: "Modbus TCP 客户端",
  modbus_tcp_slave: "Modbus TCP 从站",
};

export const MODE_LABELS: Record<GenMode, string> = {
  const: "常量",
  random: "均匀随机",
  gaussian: "高斯随机",
  increment: "递增",
  sine: "正弦波",
  enum: "枚举",
  walk: "随机游走",
  timestamp: "时间戳",
  uuid: "UUID",
  template: "模板",
};
