#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ ! -d .venv ]]; then
  python3 -m venv .venv
fi
# shellcheck disable=SC1091
source .venv/bin/activate
pip install -U pip
pip install -e ".[dev,desktop]"
pytest -q
pyinstaller --noconfirm --clean packaging/devicesim.spec
echo
echo "打包完成：$ROOT/dist/DeviceSim.app  以及  $ROOT/dist/DeviceSim"
