from __future__ import annotations

import json
import random
import threading
import time
from collections.abc import Callable
from typing import Any

import paho.mqtt.client as mqtt

from devicesim.core.data_gen import PayloadGenerator, render_template
from devicesim.core.events import InstanceStats, SimEvent
from devicesim.core.models import MqttClientConfig, SimInstance

EventCb = Callable[[SimEvent], None]


class MqttClientRunner:
    def __init__(self, instance: SimInstance, emit: EventCb):
        self.instance = instance
        self.emit = emit
        self.stats = InstanceStats()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._client: mqtt.Client | None = None
        self._connected = threading.Event()
        cfg = instance.mqtt
        if cfg is None:
            raise ValueError("MQTT 配置缺失")
        self.cfg = cfg
        self._publishers = [
            (rule, PayloadGenerator(rule.fields))
            for rule in cfg.publishes
            if rule.enabled
        ]

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        self._stop.clear()
        self.stats = InstanceStats(started_at=time.time())
        self._thread = threading.Thread(target=self._run, name=f"mqtt-{self.instance.name}", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        client = self._client
        if client is not None:
            try:
                client.disconnect()
                client.loop_stop()
            except Exception:
                pass
        if self._thread:
            self._thread.join(timeout=3)
        self.stats.connected = False

    def is_running(self) -> bool:
        return self._thread is not None and self._thread.is_alive() and not self._stop.is_set()

    def publish_once(self, rule_name: str | None = None) -> None:
        for rule, gen in self._publishers:
            if rule_name and rule.name != rule_name:
                continue
            self._publish(rule, gen)

    def _base_ctx(self) -> dict[str, Any]:
        return {
            "name": self.instance.name,
            "id": self.instance.id,
            "type": self.instance.type.value,
            "host": self.cfg.host,
            "port": self.cfg.port,
        }

    def _run(self) -> None:
        protocol = {
            "MQTTv31": mqtt.MQTTv31,
            "MQTTv311": mqtt.MQTTv311,
            "MQTTv5": mqtt.MQTTv5,
        }.get(self.cfg.protocol, mqtt.MQTTv311)
        client_id = self.cfg.client_id or f"devicesim-{self.instance.id}"
        kwargs: dict[str, Any] = {
            "callback_api_version": mqtt.CallbackAPIVersion.VERSION2,
            "client_id": client_id,
            "protocol": protocol,
        }
        if protocol != mqtt.MQTTv5:
            kwargs["clean_session"] = self.cfg.clean_session
        client = mqtt.Client(**kwargs)
        self._client = client
        if self.cfg.username:
            client.username_pw_set(self.cfg.username, self.cfg.password or None)
        if self.cfg.use_tls:
            tls_args: dict[str, Any] = {}
            if self.cfg.ca_certs:
                tls_args["ca_certs"] = self.cfg.ca_certs
            if self.cfg.certfile:
                tls_args["certfile"] = self.cfg.certfile
            if self.cfg.keyfile:
                tls_args["keyfile"] = self.cfg.keyfile
            client.tls_set(**tls_args)
            if self.cfg.tls_insecure:
                client.tls_insecure_set(True)
        if self.cfg.auto_reconnect:
            client.reconnect_delay_set(min_delay=1, max_delay=30)
        if self.cfg.lwt_topic:
            topic = render_template(self.cfg.lwt_topic, self._base_ctx())
            client.will_set(
                topic,
                payload=render_template(self.cfg.lwt_payload, self._base_ctx()),
                qos=self.cfg.lwt_qos,
                retain=self.cfg.lwt_retain,
            )

        client.on_connect = self._on_connect
        client.on_disconnect = self._on_disconnect
        client.on_message = self._on_message

        try:
            client.connect_async(self.cfg.host, int(self.cfg.port), keepalive=self.cfg.keepalive)
            client.loop_start()
        except Exception as exc:
            self._error(f"连接失败: {exc}")
            return

        due: dict[str, float] = {}
        now = time.monotonic()
        for rule, _ in self._publishers:
            delay = (rule.interval_ms + random.randint(0, max(rule.jitter_ms, 0))) / 1000
            due[rule.name] = now + delay

        while not self._stop.wait(0.05):
            now = time.monotonic()
            if not self._connected.is_set():
                continue
            for rule, gen in self._publishers:
                if now >= due.get(rule.name, 0):
                    self._publish(rule, gen)
                    jitter = random.uniform(-rule.jitter_ms, rule.jitter_ms) if rule.jitter_ms else 0
                    due[rule.name] = now + max(0.05, (rule.interval_ms + jitter) / 1000)

    def _on_connect(self, client: mqtt.Client, _userdata, _flags, reason_code, _properties=None) -> None:
        ok = getattr(reason_code, "is_failure", False) is False and str(reason_code) in {"0", "Success"}
        if hasattr(reason_code, "is_failure"):
            ok = not reason_code.is_failure
        elif isinstance(reason_code, int):
            ok = reason_code == 0
        if not ok:
            self._error(f"MQTT 拒绝连接: {reason_code}")
            return
        self._connected.set()
        self.stats.connected = True
        self._info("connect", f"已连接 {self.cfg.host}:{self.cfg.port}")
        ctx = self._base_ctx()
        if self.cfg.birth_topic:
            topic = render_template(self.cfg.birth_topic, ctx)
            client.publish(
                topic,
                payload=render_template(self.cfg.birth_payload, ctx),
                qos=self.cfg.birth_qos,
                retain=self.cfg.birth_retain,
            )
            self.stats.sent += 1
        for sub in self.cfg.subscriptions:
            if not sub.enabled:
                continue
            topic = render_template(sub.topic, ctx)
            client.subscribe(topic, qos=sub.qos)
            self._info("subscribe", f"订阅 {topic} qos={sub.qos}")

    def _on_disconnect(self, _client, _userdata, _disconnect_flags=None, reason_code=None, _properties=None) -> None:
        self._connected.clear()
        self.stats.connected = False
        self._info("disconnect", f"断开连接: {reason_code}")

    def _on_message(self, _client, _userdata, message: mqtt.MQTTMessage) -> None:
        self.stats.received += 1
        self.stats.last_activity = time.time()
        try:
            text = message.payload.decode("utf-8")
        except Exception:
            text = message.payload.hex()
        self._emit("info", "message", f"收到 {message.topic} ({len(message.payload)}B)", {"topic": message.topic, "payload": text})

    def _publish(self, rule, gen: PayloadGenerator) -> None:
        client = self._client
        if client is None or not self._connected.is_set():
            return
        ctx = self._base_ctx()
        obj = gen.next_object(ctx)
        ctx.update(obj)
        topic = render_template(rule.topic, ctx)
        if rule.payload_kind == "json":
            payload: str | bytes = json.dumps(obj, ensure_ascii=False)
        elif rule.payload_kind == "hex":
            payload = bytes.fromhex(render_template(rule.text_template, ctx).replace(" ", ""))
        else:
            payload = render_template(rule.text_template, ctx)
        try:
            info = client.publish(topic, payload=payload, qos=rule.qos, retain=rule.retain)
            if info.rc != mqtt.MQTT_ERR_SUCCESS:
                raise RuntimeError(f"publish rc={info.rc}")
            self.stats.sent += 1
            self.stats.last_activity = time.time()
            preview = payload if isinstance(payload, str) else payload.hex()
            if len(preview) > 240:
                preview = preview[:240] + "…"
            self._emit("info", "publish", f"发布 {topic} {preview}", {"topic": topic, "payload": obj})
        except Exception as exc:
            self._error(f"发布失败 {topic}: {exc}")

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
