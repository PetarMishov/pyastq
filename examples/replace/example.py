"""Small fixture for demonstrating pyastq replacements."""

import hashlib
import json
from typing import Any


def artifact_id(data: bytes) -> str:
    """Safe replacement: pyastq can rewrite this to SHA-256."""
    return hashlib.md5(data).hexdigest()


def parse_overrides(expression: str) -> dict[str, Any]:
    """Unsafe replacement: eval requires an explicit --allow-unsafe opt-in."""
    overrides = eval(expression)
    if not isinstance(overrides, dict):
        raise ValueError("overrides must evaluate to a dictionary")
    return overrides


def parse_json_overrides(document: str) -> dict[str, Any]:
    """Already clean."""
    overrides = json.loads(document)
    if not isinstance(overrides, dict):
        raise ValueError("JSON overrides must contain an object")
    return overrides
