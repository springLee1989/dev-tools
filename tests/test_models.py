from __future__ import annotations

import json

from devicesim.core.engine import Engine
from devicesim.core.models import (
    ByteOrder,
    FieldType,
    GenMode,
    InstanceType,
    ModbusRegisterDef,
    ModbusTable,
    Profile,
    as_uint,
    default_profile,
)


def test_default_profile_roundtrip():
    profile = default_profile()
    text = json.dumps(profile.model_dump(mode="json"))
    loaded = Profile.model_validate_json(text)
    assert len(loaded.instances) == 3
    types = {i.type for i in loaded.instances}
    assert types == {
        InstanceType.MQTT_CLIENT,
        InstanceType.MODBUS_TCP_CLIENT,
        InstanceType.MODBUS_TCP_SLAVE,
    }


def test_engine_add_clone_remove():
    engine = Engine()
    engine.profile.instances.clear()
    a = engine.add_default(InstanceType.MQTT_CLIENT, "alpha")
    b = engine.clone_instance(a.id)
    assert b is not None
    assert b.id != a.id
    assert len(engine.profile.instances) == 2
    engine.remove_instance(a.id)
    assert engine.get_instance(a.id) is None
    assert engine.get_instance(b.id) is not None


def test_new_register_address_is_uint():
    reg = ModbusRegisterDef(name="reg-1", address=12.0)
    assert reg.address == 12
    assert isinstance(reg.address, int)
    assert reg.data_type == FieldType.UINT16
    assert reg.field.type == FieldType.UINT16
    loaded = ModbusRegisterDef.model_validate({"name": "reg-2", "address": None})
    assert loaded.address == 0
    assert isinstance(loaded.address, int)
    assert as_uint(3.0) == 3
    assert isinstance(as_uint(3.0), int)


def test_ui_string_assignment_keeps_register_enums():
    reg = ModbusRegisterDef(
        name="reg-5",
        table=ModbusTable.DISCRETE_INPUTS,
        data_type=FieldType.BOOL,
        address=510,
    )
    reg.table = "discrete_inputs"
    reg.data_type = "bool"
    reg.byte_order = "ABCD"
    reg.field.type = "bool"
    reg.field.mode = "random"
    assert reg.table is ModbusTable.DISCRETE_INPUTS
    assert reg.data_type is FieldType.BOOL
    assert reg.byte_order is ByteOrder.ABCD
    assert reg.field.type is FieldType.BOOL
    assert reg.field.mode is GenMode.RANDOM
    assert reg.data_type == FieldType.BOOL


def test_slave_ports_unique():
    engine = Engine()
    engine.profile.instances.clear()
    x = engine.add_default(InstanceType.MODBUS_TCP_SLAVE)
    y = engine.add_default(InstanceType.MODBUS_TCP_SLAVE)
    assert x.modbus_slave is not None
    assert y.modbus_slave is not None
    assert x.modbus_slave.port != y.modbus_slave.port
