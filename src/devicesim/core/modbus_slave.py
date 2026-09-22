from __future__ import annotations

import asyncio
import random
import threading
import time
from collections.abc import Callable
from typing import Any

from pymodbus.pdu.device import ModbusDeviceIdentification
from pymodbus.server import ModbusTcpServer
from pymodbus.simulator import DataType as SimDataType
from pymodbus.simulator import SimData, SimDevice

from devicesim.core.data_gen import FieldGenerator, encode_modbus
from devicesim.core.events import InstanceStats, SimEvent
from devicesim.core.models import ModbusTable, ModbusTcpSlaveConfig, SimInstance

EventCb = Callable[[SimEvent], None]

TABLE_FC = {
    ModbusTable.COILS: 1,
    ModbusTable.DISCRETE_INPUTS: 2,
    ModbusTable.HOLDING_REGISTERS: 3,
    ModbusTable.INPUT_REGISTERS: 4,
}


class ModbusTcpSlaveRunner:
    def __init__(self, instance: SimInstance, emit: EventCb):
        self.instance = instance
        self.emit = emit
        self.stats = InstanceStats()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        cfg = instance.modbus_slave
        if cfg is None:
            raise ValueError("Modbus 从站配置缺失")
        self.cfg: ModbusTcpSlaveConfig = cfg
        self._loop: asyncio.AbstractEventLoop | None = None
        self._server: ModbusTcpServer | None = None
        self._gens = [(reg, FieldGenerator(reg.field)) for reg in cfg.registers if reg.enabled]

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        self._stop.clear()
        self.stats = InstanceStats(started_at=time.time())
        self._thread = threading.Thread(target=self._thread_main, name=f"mbs-{self.instance.name}", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        loop = self._loop
        server = self._server
        if loop and server is not None and loop.is_running():
            try:
                fut = asyncio.run_coroutine_threadsafe(server.shutdown(), loop)
                fut.result(timeout=3)
            except Exception:
                try:
                    loop.call_soon_threadsafe(loop.stop)
                except Exception:
                    pass
        if self._thread:
            self._thread.join(timeout=4)
        self.stats.connected = False

    def is_running(self) -> bool:
        return self._thread is not None and self._thread.is_alive() and not self._stop.is_set()

    def _thread_main(self) -> None:
        loop = asyncio.new_event_loop()
        self._loop = loop
        asyncio.set_event_loop(loop)
        try:
            loop.run_until_complete(self._amain())
        except Exception as exc:
            self._error(f"从站退出: {exc}")
        finally:
            pending = asyncio.all_tasks(loop)
            for task in pending:
                task.cancel()
            try:
                loop.run_until_complete(asyncio.gather(*pending, return_exceptions=True))
            except Exception:
                pass
            loop.close()

    async def _amain(self) -> None:
        cfg = self.cfg
        identity = ModbusDeviceIdentification()
        identity.VendorName = cfg.identity_vendor
        identity.ProductCode = "DS"
        identity.ProductName = cfg.identity_product
        identity.ModelName = self.instance.name
        identity.MajorMinorRevision = cfg.identity_version

        device = SimDevice(
            id=int(cfg.unit_id),
            identity=identity,
            simdata=(
                [SimData(address=0, count=max(16, cfg.coils_size), values=False, datatype=SimDataType.BITS)],
                [SimData(address=0, count=max(16, cfg.discrete_inputs_size), values=False, datatype=SimDataType.BITS)],
                [SimData(address=0, count=max(1, cfg.holding_registers_size), values=0, datatype=SimDataType.REGISTERS)],
                [SimData(address=0, count=max(1, cfg.input_registers_size), values=0, datatype=SimDataType.REGISTERS)],
            ),
        )
        address = (cfg.host, int(cfg.port))
        self._server = ModbusTcpServer(context=device, identity=identity, address=address)
        await self._server.serve_forever(background=True)
        self.stats.connected = True
        self._info("listen", f"从站监听 {cfg.host}:{cfg.port} unit={cfg.unit_id}")
        try:
            await self._update_loop()
        finally:
            if self._server is not None:
                await self._server.shutdown()

    async def _update_loop(self) -> None:
        due: dict[str, float] = {}
        now = time.monotonic()
        for reg, _ in self._gens:
            due[reg.name] = now
        while not self._stop.is_set():
            now = time.monotonic()
            for reg, gen in self._gens:
                if now < due.get(reg.name, 0):
                    continue
                value = gen.next({"name": self.instance.name, "id": self.instance.id})
                encoded = encode_modbus(value, reg.data_type, reg.byte_order)
                bool_mode = reg.data_type.value == "bool"
                payload: list[Any] = [bool(v) for v in encoded] if bool_mode else encoded
                await self._write_value(reg.table, reg.address, payload)
                self.stats.sent += 1
                self.stats.last_activity = time.time()
                self._emit("info", "generate", f"{reg.table.value}[{reg.address}] {reg.name}={value}", value)
                jitter = random.uniform(-reg.jitter_ms, reg.jitter_ms) if reg.jitter_ms else 0
                due[reg.name] = now + max(0.05, (reg.interval_ms + jitter) / 1000)
            await asyncio.sleep(0.05)

    async def _write_value(self, table: ModbusTable, address: int, values: list[Any]) -> None:
        server = self._server
        if server is None:
            return
        fc = TABLE_FC[table]
        try:
            err = await server.context.async_setValues(self.cfg.unit_id, fc, address, values)
            if err:
                self._error(f"写入数据区失败 {table.value}[{address}]: {err}")
        except Exception as exc:
            self._error(f"写入数据区失败 {table.value}[{address}]: {exc}")

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
