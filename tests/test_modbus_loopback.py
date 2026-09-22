from __future__ import annotations

import time

from devicesim.core.engine import Engine
from devicesim.core.models import (
    DataField,
    FieldType,
    GenMode,
    InstanceType,
    ModbusFunction,
    ModbusOperation,
    ModbusRegisterDef,
    ModbusTable,
    ModbusTcpClientConfig,
    ModbusTcpSlaveConfig,
    SimInstance,
)


def test_modbus_client_reads_slave_generated_register():
    port = 15020
    engine = Engine()
    engine.profile.instances.clear()
    slave = SimInstance(
        name="slave",
        type=InstanceType.MODBUS_TCP_SLAVE,
        modbus_slave=ModbusTcpSlaveConfig(
            host="127.0.0.1",
            port=port,
            registers=[
                ModbusRegisterDef(
                    name="sp",
                    table=ModbusTable.HOLDING_REGISTERS,
                    address=0,
                    data_type=FieldType.UINT16,
                    interval_ms=100,
                    field=DataField(
                        name="sp",
                        type=FieldType.UINT16,
                        mode=GenMode.CONST,
                        value=1234,
                        decimals=0,
                    ),
                )
            ],
        ),
    )
    master = SimInstance(
        name="master",
        type=InstanceType.MODBUS_TCP_CLIENT,
        modbus_client=ModbusTcpClientConfig(
            host="127.0.0.1",
            port=port,
            operations=[
                ModbusOperation(
                    name="rd",
                    function=ModbusFunction.READ_HOLDING_REGISTERS,
                    address=0,
                    count=1,
                    interval_ms=150,
                )
            ],
        ),
    )
    engine.add_instance(slave)
    engine.add_instance(master)
    events = []
    engine.set_emit(events.append)
    try:
        engine.start_instance(slave.id)
        time.sleep(0.4)
        engine.start_instance(master.id)
        deadline = time.time() + 4
        ok = False
        while time.time() < deadline:
            if any(e.kind == "read" and e.payload and 1234 in (e.payload if isinstance(e.payload, list) else [e.payload]) for e in events):
                ok = True
                break
            time.sleep(0.1)
        assert ok, f"未读到从站数据: {[e.message for e in events[-10:]]}"
        assert engine.stats_of(master.id).received >= 1
    finally:
        engine.stop_all()
