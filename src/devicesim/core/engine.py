from __future__ import annotations

import json
import threading
from collections.abc import Callable
from pathlib import Path
from typing import Any

from devicesim.core.events import InstanceStats, SimEvent
from devicesim.core.modbus_client import ModbusTcpClientRunner
from devicesim.core.modbus_slave import ModbusTcpSlaveRunner
from devicesim.core.models import (
    InstanceType,
    Profile,
    SimInstance,
    default_modbus_client_instance,
    default_modbus_slave_instance,
    default_mqtt_instance,
    default_profile,
)
from devicesim.core.mqtt_client import MqttClientRunner

EventCb = Callable[[SimEvent], None]
Runner = MqttClientRunner | ModbusTcpClientRunner | ModbusTcpSlaveRunner


class Engine:
    """管理多实例并发仿真。"""

    def __init__(self, emit: EventCb | None = None):
        self._emit = emit or (lambda _e: None)
        self._lock = threading.RLock()
        self.profile = default_profile()
        self._runners: dict[str, Runner] = {}
        self.profile_path: Path | None = None

    def set_emit(self, emit: EventCb) -> None:
        self._emit = emit

    def emit(self, event: SimEvent) -> None:
        self._emit(event)

    def load_profile(self, path: str | Path) -> Profile:
        path = Path(path)
        data = json.loads(path.read_text(encoding="utf-8"))
        profile = Profile.model_validate(data)
        self.stop_all()
        with self._lock:
            self.profile = profile
            self.profile_path = path
        return profile

    def load_profile_data(self, data: dict[str, Any] | str) -> Profile:
        profile = Profile.model_validate_json(data) if isinstance(data, str) else Profile.model_validate(data)
        self.stop_all()
        with self._lock:
            self.profile = profile
        return profile

    def save_profile(self, path: str | Path | None = None) -> Path:
        path = Path(path) if path else self.profile_path
        if path is None:
            raise ValueError("未指定保存路径")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            json.dumps(self.profile.model_dump(mode="json"), ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
        self.profile_path = path
        return path

    def dump_profile(self) -> str:
        return json.dumps(self.profile.model_dump(mode="json"), ensure_ascii=False, indent=2)

    def add_instance(self, instance: SimInstance) -> SimInstance:
        with self._lock:
            self.profile.instances.append(instance)
        return instance

    def add_default(self, type_: InstanceType, name: str | None = None) -> SimInstance:
        if type_ == InstanceType.MQTT_CLIENT:
            inst = default_mqtt_instance(name or f"mqtt-{len(self.profile.instances) + 1}")
        elif type_ == InstanceType.MODBUS_TCP_CLIENT:
            inst = default_modbus_client_instance(name or f"mb-master-{len(self.profile.instances) + 1}")
        else:
            used_ports = {
                i.modbus_slave.port
                for i in self.profile.instances
                if i.modbus_slave is not None
            }
            port = 5020
            while port in used_ports:
                port += 1
            inst = default_modbus_slave_instance(name or f"mb-device-{len(self.profile.instances) + 1}", port=port)
        return self.add_instance(inst)

    def remove_instance(self, instance_id: str) -> None:
        self.stop_instance(instance_id)
        with self._lock:
            self.profile.instances = [i for i in self.profile.instances if i.id != instance_id]

    def clone_instance(self, instance_id: str) -> SimInstance | None:
        src = self.get_instance(instance_id)
        if src is None:
            return None
        data = src.model_dump()
        data.pop("id", None)
        data["name"] = f"{src.name}-copy"
        cloned = SimInstance.model_validate(data)
        if cloned.modbus_slave is not None:
            used = {i.modbus_slave.port for i in self.profile.instances if i.modbus_slave}
            port = cloned.modbus_slave.port + 1
            while port in used:
                port += 1
            cloned.modbus_slave.port = port
        return self.add_instance(cloned)

    def get_instance(self, instance_id: str) -> SimInstance | None:
        for inst in self.profile.instances:
            if inst.id == instance_id:
                return inst
        return None

    def replace_instance(self, instance: SimInstance) -> None:
        running = self.is_running(instance.id)
        if running:
            self.stop_instance(instance.id)
        with self._lock:
            self.profile.instances = [instance if i.id == instance.id else i for i in self.profile.instances]
        if running:
            self.start_instance(instance.id)

    def start_instance(self, instance_id: str) -> None:
        inst = self.get_instance(instance_id)
        if inst is None:
            raise KeyError(instance_id)
        if not inst.enabled:
            raise RuntimeError(f"{inst.name} 已禁用")
        with self._lock:
            existing = self._runners.get(instance_id)
            if existing and existing.is_running():
                return
            runner = self._make_runner(inst)
            self._runners[instance_id] = runner
        runner.start()

    def stop_instance(self, instance_id: str) -> None:
        with self._lock:
            runner = self._runners.pop(instance_id, None)
        if runner:
            runner.stop()

    def start_all(self) -> None:
        for inst in list(self.profile.instances):
            if inst.enabled:
                self.start_instance(inst.id)

    def stop_all(self) -> None:
        with self._lock:
            ids = list(self._runners)
        for instance_id in ids:
            self.stop_instance(instance_id)

    def is_running(self, instance_id: str) -> bool:
        runner = self._runners.get(instance_id)
        return bool(runner and runner.is_running())

    def stats_of(self, instance_id: str) -> InstanceStats:
        runner = self._runners.get(instance_id)
        if runner:
            return runner.stats
        return InstanceStats()

    def all_stats(self) -> dict[str, InstanceStats]:
        return {i.id: self.stats_of(i.id) for i in self.profile.instances}

    def trigger_once(self, instance_id: str, rule_name: str | None = None) -> None:
        runner = self._runners.get(instance_id)
        if runner is None:
            raise RuntimeError("实例未运行")
        if isinstance(runner, MqttClientRunner):
            runner.publish_once(rule_name)
        elif isinstance(runner, ModbusTcpClientRunner):
            runner.trigger_once(rule_name)

    def _make_runner(self, inst: SimInstance) -> Runner:
        if inst.type == InstanceType.MQTT_CLIENT:
            return MqttClientRunner(inst, self.emit)
        if inst.type == InstanceType.MODBUS_TCP_CLIENT:
            return ModbusTcpClientRunner(inst, self.emit)
        return ModbusTcpSlaveRunner(inst, self.emit)
