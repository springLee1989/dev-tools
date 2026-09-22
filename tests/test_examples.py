from __future__ import annotations

from pathlib import Path

from devicesim.core.models import Profile

EXAMPLES = Path(__file__).resolve().parents[1] / "examples"


def test_examples_validate():
    files = list(EXAMPLES.glob("*.json"))
    assert files, "缺少 examples/*.json"
    for path in files:
        Profile.model_validate_json(path.read_text(encoding="utf-8"))
