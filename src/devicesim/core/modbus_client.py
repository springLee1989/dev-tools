from __future__ import annotations

import random
import threading
import time
from collections.abc import Callable
from typing import Any

from pymodbus.client import ModbusTcpClient
from pymodbus.exceptions import ModbusException

from devicesim.core.data_gen import PayloadGenerator, decode_modbus, encode_modbus, register_count
from devicesim.core.events import InstanceStats, SimEvent
from devicesim.core.models import (
    FieldType,
    ModbusFunction,
    ModbusOperation,
    ModbusTcpClientConfig,
    SimInstance,
)

EventCb = Callable[[SimEvent], None]


def _result_ok(result: Any) -> bool:
    return result is not None and not getattr(result, "isError", lambda: False)()


class ModbusTcpClientRunner:
    def __init__(self, instance: SimInstance, emit: EventCb):
        self.instance = instance
        self.emit = emit
        self.stats = InstanceStats()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        cfg = instance.modbus_client
        if cfg is None:
            raise ValueError("Modbus 客户端配置缺失")
        self.cfg: ModbusTcpClientConfig = cfg
        self._client: ModbusTcpClient | None = None

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        self._stop.clear()
        self.stats = InstanceStats(started_at=time.time())
        self._thread = threading.Thread(target=self._run, name=f"mbc-{self.instance.name}", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._client is not None:
            try:
                self._client.close()
            except Exception:
                pass
        if self._thread:
            self._thread.join(timeout=3)
        self.stats.connected = False

    def is_running(self) -> bool:
        return self._thread is not None and self._thread.is_alive() and not self._stop.is_set()

    def trigger_once(self, op_name: str | None = None) -> None:
        client = self._ensure_client()
        if client is None:
            return
        for op in self.cfg.operations:
            if op_name and op.name != op_name:
                continue
            if op.enabled:
                self._execute(client, op)

    def _ensure_client(self) -> ModbusTcpClient | None:
        if self._client and self._client.connected:
            return self._client
        client = ModbusTcpClient(self.cfg.host, port=int(self.cfg.port), timeout=self.cfg.timeout)
        ok = client.connect()
        self._client = client
        self.stats.connected = bool(ok)
        if not ok:
            self._error(f"无法连接 {self.cfg.host}:{self.cfg.port}")
            return None
        self._info("connect", f"已连接 {self.cfg.host}:{self.cfg.port} unit={self.cfg.unit_id}")
        return client

    def _run(self) -> None:
        due: dict[str, float] = {}
        now = time.monotonic()
        for op in self.cfg.operations:
            due[op.name] = now + (op.interval_ms / 1000)

        while not self._stop.wait(0.05):
            client = self._ensure_client()
            if client is None:
                if not self.cfg.auto_reconnect:
                    break
                self._stop.wait(1.0)
                continue
            now = time.monotonic()
            for op in self.cfg.operations:
                if not op.enabled:
                    continue
                if now >= due.get(op.name, 0):
                    self._execute(client, op)
                    jitter = random.uniform(-op.jitter_ms, op.jitter_ms) if op.jitter_ms else 0
                    due[op.name] = now + max(0.05, (op.interval_ms + jitter) / 1000)

    def _execute(self, client: ModbusTcpClient, op: ModbusOperation) -> None:
        unit = self.cfg.unit_id
        fn = op.function
        try:
            if fn in {
                ModbusFunction.WRITE_COIL,
                ModbusFunction.WRITE_REGISTER,
                ModbusFunction.WRITE_COILS,
                ModbusFunction.WRITE_REGISTERS,
            }:
                values = self._gen_write_values(op)
                result = self._do_write(client, op, values, unit)
                if not _result_ok(result):
                    raise ModbusException(str(result))
                self.stats.sent += 1
                self.stats.last_activity = time.time()
                self._info("write", f"{fn.value} addr={op.address} values={values}")
            else:
                result = self._do_read(client, op, unit)
                if not _result_ok(result):
                    raise ModbusException(str(result))
                decoded = self._decode_read(op, result)
                self.stats.received += 1
                self.stats.last_activity = time.time()
                self._emit("info", "read", f"{fn.value} addr={op.address} => {decoded}", decoded)
        except Exception as exc:
            self.stats.connected = bool(getattr(client, "connected", False))
            self._error(f"{op.name} 失败: {exc}")
            if self.cfg.auto_reconnect:
                try:
                    client.close()
                except Exception:
                    pass
                self._client = None
                self.stats.connected = False

    def _gen_write_values(self, op: ModbusOperation) -> list[Any]:
        if op.fields:
            obj = PayloadGenerator(op.fields).next_object({"name": self.instance.name, "id": self.instance.id})
            values: list[Any] = []
            for field in op.fields:
                encoded = encode_modbus(obj[field.name], field.type, op.byte_order)
                if op.function in {ModbusFunction.WRITE_COIL, ModbusFunction.WRITE_COILS}:
                    values.append(bool(obj[field.name]))
                else:
                    values.extend(encoded)
            if op.function in {ModbusFunction.WRITE_COIL, ModbusFunction.WRITE_REGISTER}:
                return values[:1]
            return values
        if op.function in {ModbusFunction.WRITE_COIL, ModbusFunction.WRITE_COILS}:
            return [True] * max(1, op.count)
        return [0] * max(1, op.count)

    def _call(self, client: ModbusTcpClient, method: str, address: int, unit: int, **kwargs):
        fn = getattr(client, method)
        try:
            return fn(address, device_id=unit, **kwargs)
        except TypeError:
            try:
                return fn(address, slave=unit, **kwargs)
            except TypeError:
                return fn(address, **kwargs)

    def _do_read(self, client: ModbusTcpClient, op: ModbusOperation, unit: int):
        count = max(1, op.count)
        mapping = {
            ModbusFunction.READ_COILS: "read_coils",
            ModbusFunction.READ_DISCRETE_INPUTS: "read_discrete_inputs",
            ModbusFunction.READ_HOLDING_REGISTERS: "read_holding_registers",
            ModbusFunction.READ_INPUT_REGISTERS: "read_input_registers",
        }
        return self._call(client, mapping[op.function], op.address, unit, count=count)

    def _do_write(self, client: ModbusTcpClient, op: ModbusOperation, values: list[Any], unit: int):
        if op.function == ModbusFunction.WRITE_COIL:
            return self._call(client, "write_coil", op.address, unit, value=bool(values[0]))
        if op.function == ModbusFunction.WRITE_REGISTER:
            return self._call(client, "write_register", op.address, unit, value=int(values[0]) & 0xFFFF)
        if op.function == ModbusFunction.WRITE_COILS:
            return self._call(client, "write_coils", op.address, unit, values=[bool(v) for v in values])
        return self._call(client, "write_registers", op.address, unit, values=[int(v) & 0xFFFF for v in values])

    def _decode_read(self, op: ModbusOperation, result: Any) -> Any:
        if op.function in {ModbusFunction.READ_COILS, ModbusFunction.READ_DISCRETE_INPUTS}:
            bits = list(getattr(result, "bits", None) or [])
            return [bool(x) for x in bits[: op.count]]
        registers = list(getattr(result, "registers", None) or [])
        n = register_count(op.data_type)
        if n == 1 or op.data_type in {FieldType.BOOL, FieldType.UINT16, FieldType.INT16, FieldType.INT}:
            if op.data_type == FieldType.INT16:
                return [decode_modbus([r], op.data_type, op.byte_order) for r in registers]
            return registers
        out = []
        for i in range(0, len(registers) - n + 1, n):
            out.append(decode_modbus(registers[i : i + n], op.data_type, op.byte_order))
        return out

    def _info(self, kind: str, message: str) -> None:
        self._emit("info", kind, message)

    def _error(self, message: str) -> None:
        self.stats.errors += 1
        self.stats.last_error = message
        self._emit("error", "error", message)

    def _emit(self, level: str, kind: str, message: str, payload: Any = None) -> None:
        self.emit(
            SimEvent(
                ts=time.time(),
                instance_id=self.instance.id,
                instance_name=self.instance.name,
                level=level,
                kind=kind,
                message=message,
                payload=payload,
            )
        )
