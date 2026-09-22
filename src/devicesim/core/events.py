from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class SimEvent:
    ts: float
    instance_id: str
    instance_name: str
    level: str
    kind: str
    message: str
    payload: Any = None


@dataclass
class InstanceStats:
    sent: int = 0
    received: int = 0
    errors: int = 0
    connected: bool = False
    last_error: str = ""
    started_at: float | None = None
    last_activity: float | None = None
    extra: dict[str, Any] = field(default_factory=dict)
