from __future__ import annotations

import argparse
import json
import signal
import sys
import time
from pathlib import Path

from devicesim.core.engine import Engine
from devicesim.core.events import SimEvent
from devicesim.core.models import Profile
from devicesim.version import __version__


def _print_event(event: SimEvent) -> None:
    ts = time.strftime("%H:%M:%S", time.localtime(event.ts))
    print(f"{ts} [{event.level:<5}] {event.instance_name:<20} {event.kind:<10} {event.message}", flush=True)


def cmd_run(args: argparse.Namespace) -> int:
    engine = Engine(emit=_print_event)
    engine.load_profile(args.profile)
    print(f"已加载方案: {engine.profile.name} ({len(engine.profile.instances)} 个实例)", flush=True)
    engine.start_all()

    def _stop(_signum=None, _frame=None):
        print("正在停止…", flush=True)
        engine.stop_all()
        sys.exit(0)

    signal.signal(signal.SIGINT, _stop)
    signal.signal(signal.SIGTERM, _stop)
    duration = args.duration
    started = time.time()
    try:
        while True:
            if duration and time.time() - started >= duration:
                break
            time.sleep(0.5)
    finally:
        engine.stop_all()
    return 0


def cmd_validate(args: argparse.Namespace) -> int:
    path = Path(args.profile)
    Profile.model_validate_json(path.read_text(encoding="utf-8"))
    print(f"方案有效: {path}")
    return 0


def cmd_schema(_args: argparse.Namespace) -> int:
    print(json.dumps(Profile.model_json_schema(), ensure_ascii=False, indent=2))
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="devicesim",
        description="MQTT / Modbus TCP 多实例设备仿真工具",
    )
    parser.add_argument("--version", action="version", version=f"DeviceSim {__version__}")
    sub = parser.add_subparsers(dest="command")

    gui = sub.add_parser("gui", help="启动图形界面（默认）")
    gui.add_argument("--host", default="127.0.0.1")
    gui.add_argument("--port", type=int, default=18765)
    gui.add_argument("--native", action="store_true", help="使用系统原生窗口")
    gui.add_argument("--profile", help="启动时加载的方案文件")

    run = sub.add_parser("run", help="无界面运行方案")
    run.add_argument("profile", help="方案 JSON 路径")
    run.add_argument("--duration", type=float, default=0, help="运行秒数，0 表示直到 Ctrl+C")

    val = sub.add_parser("validate", help="校验方案 JSON")
    val.add_argument("profile")

    sub.add_parser("schema", help="打印方案 JSON Schema")
    return parser


def cli_main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.command in (None, "gui"):
        from devicesim.ui.app import run_gui

        host = getattr(args, "host", "127.0.0.1")
        port = getattr(args, "port", 18765)
        native = getattr(args, "native", False)
        profile = getattr(args, "profile", None)
        run_gui(host=host, port=port, native=native, profile=profile)
        return 0
    if args.command == "run":
        return cmd_run(args)
    if args.command == "validate":
        return cmd_validate(args)
    if args.command == "schema":
        return cmd_schema(args)
    parser.print_help()
    return 1
