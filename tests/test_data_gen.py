from __future__ import annotations

from devicesim.core.data_gen import FieldGenerator, PayloadGenerator, decode_modbus, encode_modbus, render_template
from devicesim.core.models import ByteOrder, DataField, FieldType, GenMode


def test_const_and_cast():
    gen = FieldGenerator(DataField(name="n", type=FieldType.INT16, mode=GenMode.CONST, value=12))
    assert gen.next() == 12


def test_random_range():
    field = DataField(name="x", type=FieldType.INT, mode=GenMode.RANDOM, min=10, max=20, decimals=0)
    gen = FieldGenerator(field)
    values = {gen.next() for _ in range(80)}
    assert min(values) >= 10
    assert max(values) <= 20


def test_increment_wrap():
    field = DataField(name="c", type=FieldType.INT, mode=GenMode.INCREMENT, start=0, step=1, min=0, max=2, wrap=True)
    gen = FieldGenerator(field)
    assert [gen.next() for _ in range(5)] == [0, 1, 2, 0, 1]


def test_enum_values():
    field = DataField(name="s", type=FieldType.STRING, mode=GenMode.ENUM, values=["a", "b"])
    gen = FieldGenerator(field)
    assert {gen.next() for _ in range(30)} <= {"a", "b"}


def test_template_and_payload():
    fields = [
        DataField(name="temperature", type=FieldType.FLOAT, mode=GenMode.CONST, value=21.5, decimals=1),
        DataField(name="ts", type=FieldType.STRING, mode=GenMode.TIMESTAMP),
    ]
    payload = PayloadGenerator(fields).next_object({"name": "dev-1"})
    assert payload["temperature"] == 21.5
    assert "T" in payload["ts"]
    topic = render_template("devices/${name}/t/${temperature}", {"name": "dev-1", **payload})
    assert topic == "devices/dev-1/t/21.5"


def test_modbus_float_roundtrip():
    regs = encode_modbus(12.5, FieldType.FLOAT32, ByteOrder.ABCD)
    assert len(regs) == 2
    value = decode_modbus(regs, FieldType.FLOAT32, ByteOrder.ABCD)
    assert abs(value - 12.5) < 1e-5


def test_modbus_byte_orders():
    regs = encode_modbus(0x12345678, FieldType.UINT32, ByteOrder.CDAB)
    value = decode_modbus(regs, FieldType.UINT32, ByteOrder.CDAB)
    assert value == 0x12345678


def test_walk_stays_in_range():
    field = DataField(name="t", type=FieldType.FLOAT, mode=GenMode.WALK, min=18, max=22, start=20, step=0.5, decimals=2)
    gen = FieldGenerator(field)
    for _ in range(200):
        v = gen.next()
        assert 18 <= v <= 22
