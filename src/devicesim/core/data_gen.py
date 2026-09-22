from __future__ import annotations

import math
import random
import re
import struct
import time
import uuid
from datetime import datetime, timezone
from typing import Any

from devicesim.core.models import ByteOrder, DataField, FieldType, GenMode

_TOKEN_RE = re.compile(r"\$\{([a-zA-Z0-9_]+)\}|\{\{([a-zA-Z0-9_]+)\}\}")

INT_RANGES = {
    FieldType.INT16: (-32768, 32767),
    FieldType.UINT16: (0, 65535),
    FieldType.INT32: (-2147483648, 2147483647),
    FieldType.UINT32: (0, 4294967295),
}


def render_template(template: str, ctx: dict[str, Any]) -> str:
    def _repl(match: re.Match[str]) -> str:
        key = match.group(1) or match.group(2)
        if key not in ctx or ctx[key] is None:
            return match.group(0)
        return str(ctx[key])

    return _TOKEN_RE.sub(_repl, template)


def clamp_numeric(value: float, field: DataField) -> float:
    lo, hi = field.min, field.max
    if field.type in INT_RANGES:
        rlo, rhi = INT_RANGES[field.type]
        lo = rlo if lo is None else max(rlo, lo)
        hi = rhi if hi is None else min(rhi, hi)
    if lo is not None:
        value = max(value, lo)
    if hi is not None:
        value = min(value, hi)
    return value


def cast_value(value: Any, field: DataField) -> Any:
    if field.mode == GenMode.TIMESTAMP:
        return value
    if field.mode == GenMode.UUID:
        return str(value)
    if field.type == FieldType.BOOL:
        if isinstance(value, str):
            return value.lower() in {"1", "true", "yes", "on"}
        return bool(value)
    if field.type == FieldType.STRING:
        return str(value)
    if field.type == FieldType.HEX:
        if isinstance(value, bytes):
            return value.hex()
        return str(value)
    number = float(value)
    if field.type in {FieldType.FLOAT, FieldType.FLOAT32}:
        if field.decimals is not None:
            return round(number, field.decimals)
        return number
    number = clamp_numeric(number, field)
    return int(round(number))


class FieldGenerator:
    def __init__(self, field: DataField):
        self.field = field
        self._n = float(field.start)
        self._t0 = time.monotonic()
        if field.mode == GenMode.WALK:
            self._n = float(field.start if field.start is not None else (field.min or 0))

    def next(self, ctx: dict[str, Any] | None = None) -> Any:
        ctx = ctx or {}
        field = self.field
        mode = field.mode

        if mode == GenMode.CONST:
            raw = field.value
        elif mode == GenMode.RANDOM:
            raw = self._random()
        elif mode == GenMode.GAUSSIAN:
            mu = field.mean if field.mean is not None else (
                ((field.min or 0) + (field.max or 0)) / 2
            )
            sigma = field.std if field.std is not None else 1.0
            raw = random.gauss(mu, sigma)
            raw = clamp_numeric(raw, field)
        elif mode == GenMode.INCREMENT:
            raw = self._n
            self._n += field.step
            if field.wrap and field.max is not None and self._n > field.max:
                self._n = field.min if field.min is not None else field.start
        elif mode == GenMode.SINE:
            elapsed = time.monotonic() - self._t0
            period = field.period or 60
            lo = field.min if field.min is not None else 0
            hi = field.max if field.max is not None else 1
            mid = (lo + hi) / 2
            amp = (hi - lo) / 2
            raw = mid + amp * math.sin(2 * math.pi * elapsed / period)
        elif mode == GenMode.ENUM:
            values = field.values or [field.value]
            if not values:
                raw = None
            elif field.weights and len(field.weights) == len(values):
                raw = random.choices(values, weights=field.weights, k=1)[0]
            else:
                raw = random.choice(values)
        elif mode == GenMode.WALK:
            delta = random.uniform(-abs(field.step), abs(field.step))
            self._n = clamp_numeric(self._n + delta, field)
            raw = self._n
        elif mode == GenMode.TIMESTAMP:
            now = datetime.now(timezone.utc)
            fmt = field.template or "iso8601"
            if fmt == "iso8601":
                raw = now.isoformat(timespec="milliseconds").replace("+00:00", "Z")
            elif fmt == "epoch":
                raw = int(now.timestamp())
            elif fmt == "epoch_ms":
                raw = int(now.timestamp() * 1000)
            else:
                raw = now.strftime(fmt)
        elif mode == GenMode.UUID:
            raw = str(uuid.uuid4())
        elif mode == GenMode.TEMPLATE:
            raw = render_template(field.template or field.value or "", ctx)
        else:
            raw = field.value

        if field.type == FieldType.BOOL and mode == GenMode.RANDOM:
            raw = random.random() < field.probability
        if field.type == FieldType.HEX and mode in {GenMode.RANDOM, GenMode.UUID}:
            raw = random.randbytes(max(1, field.hex_bytes)).hex()
        if field.type == FieldType.STRING and mode == GenMode.RANDOM and not field.template:
            raw = f"{field.name}-{random.randint(0, 999999):06d}"

        return cast_value(raw, field)

    def _random(self) -> Any:
        field = self.field
        if field.type == FieldType.BOOL:
            return random.random() < field.probability
        lo = field.min if field.min is not None else 0
        hi = field.max if field.max is not None else lo + 1
        if hi < lo:
            lo, hi = hi, lo
        if field.type in {FieldType.FLOAT, FieldType.FLOAT32, FieldType.STRING}:
            return random.uniform(lo, hi)
        return random.randint(int(round(lo)), int(round(hi)))


class PayloadGenerator:
    def __init__(self, fields: list[DataField]):
        self.fields = fields
        self.gens = [FieldGenerator(f) for f in fields]

    def next_object(self, ctx: dict[str, Any] | None = None) -> dict[str, Any]:
        base = dict(ctx or {})
        out: dict[str, Any] = {}
        for field, gen in zip(self.fields, self.gens):
            value = gen.next(base)
            out[field.name] = value
            base[field.name] = value
        return out


def encode_modbus(value: Any, data_type: FieldType, byte_order: ByteOrder) -> list[int]:
    if data_type == FieldType.BOOL:
        return [1 if value else 0]
    if data_type in {FieldType.INT16, FieldType.UINT16, FieldType.INT}:
        unsigned = int(value) & 0xFFFF
        return [unsigned]
    if data_type == FieldType.STRING:
        data = str(value).encode("utf-8")
        if len(data) % 2:
            data += b"\x00"
        regs = []
        for i in range(0, len(data), 2):
            regs.append((data[i] << 8) | data[i + 1])
        return regs or [0]

    if data_type == FieldType.INT32:
        raw = struct.pack(">i", int(value))
    elif data_type == FieldType.UINT32:
        raw = struct.pack(">I", int(value) & 0xFFFFFFFF)
    elif data_type in {FieldType.FLOAT, FieldType.FLOAT32}:
        raw = struct.pack(">f", float(value))
    else:
        return [int(value) & 0xFFFF]

    return _bytes_to_regs32(raw, byte_order)


def decode_modbus(registers: list[int], data_type: FieldType, byte_order: ByteOrder) -> Any:
    if data_type == FieldType.BOOL:
        return bool(registers[0] if registers else 0)
    if data_type in {FieldType.UINT16, FieldType.INT}:
        return registers[0] if registers else 0
    if data_type == FieldType.INT16:
        v = registers[0] if registers else 0
        return v - 0x10000 if v & 0x8000 else v
    if data_type == FieldType.STRING:
        data = b"".join(int(r & 0xFFFF).to_bytes(2, "big") for r in registers)
        return data.split(b"\x00", 1)[0].decode("utf-8", errors="replace")
    raw = _regs32_to_bytes(registers[:2], byte_order)
    if data_type == FieldType.INT32:
        return struct.unpack(">i", raw)[0]
    if data_type == FieldType.UINT32:
        return struct.unpack(">I", raw)[0]
    return struct.unpack(">f", raw)[0]


def register_count(data_type: FieldType) -> int:
    if data_type in {FieldType.INT32, FieldType.UINT32, FieldType.FLOAT, FieldType.FLOAT32}:
        return 2
    return 1


def _bytes_to_regs32(raw: bytes, byte_order: ByteOrder) -> list[int]:
    b = _permute(raw, byte_order)
    return [(b[0] << 8) | b[1], (b[2] << 8) | b[3]]


def _regs32_to_bytes(registers: list[int], byte_order: ByteOrder) -> bytes:
    while len(registers) < 2:
        registers.append(0)
    raw = bytes(
        [
            (registers[0] >> 8) & 0xFF,
            registers[0] & 0xFF,
            (registers[1] >> 8) & 0xFF,
            registers[1] & 0xFF,
        ]
    )
    return _unpermute(raw, byte_order)


def _permute(abcd: bytes, byte_order: ByteOrder) -> bytes:
    a, b, c, d = abcd
    mapping = {
        ByteOrder.ABCD: bytes([a, b, c, d]),
        ByteOrder.CDAB: bytes([c, d, a, b]),
        ByteOrder.BADC: bytes([b, a, d, c]),
        ByteOrder.DCBA: bytes([d, c, b, a]),
    }
    return mapping[byte_order]


def _unpermute(data: bytes, byte_order: ByteOrder) -> bytes:
    labels = _permute(b"ABCD", byte_order)
    recovered = bytearray(4)
    for i, label in enumerate(labels):
        recovered[b"ABCD".index(label)] = data[i]
    return bytes(recovered)
