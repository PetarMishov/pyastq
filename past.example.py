"""Example input for ``past.example.toml``.

This module intentionally mixes code that should be reported with similar code
that should remain clean. It is meant to be analyzed, not executed.
"""

import ast
import json
from dataclasses import dataclass
from typing import Any

import requests


DEFAULT_TIMEOUT = 10


def parse_json_document(value: str) -> Any:
    """Safe parsing: this is not a call to eval."""
    return json.loads(value)


def parse_python_literal(value: str) -> Any:
    """Safe parsing: literal_eval must not match the exact call name eval."""
    return ast.literal_eval(value)


def evaluate_expression(value: str) -> Any:
    """Violation: the no-eval rule should report this call."""
    return eval(value)


def evaluate_trusted_fixture(value: str) -> Any:
    """Suppressed exception: this eval should not be reported."""
    return eval(value)  # past: ignore no-eval


def fetch_profile(user_id: int) -> dict[str, Any]:
    """Violation: requests.get has no timeout keyword."""
    response = requests.get(f"https://example.test/users/{user_id}")
    response.raise_for_status()
    return response.json()


def fetch_settings(user_id: int) -> dict[str, Any]:
    """Compliant: a timeout keyword is present."""
    response = requests.get(
        f"https://example.test/users/{user_id}/settings",
        timeout=DEFAULT_TIMEOUT,
    )
    response.raise_for_status()
    return response.json()


def fetch_healthcheck() -> bool:
    """Compliant: timeout=None still satisfies this structural rule."""
    response = requests.get("https://example.test/health", timeout=None)
    return response.ok


def fetch_with_options(url: str, options: dict[str, Any]) -> requests.Response:
    """Violation: **options may contain a timeout, but no timeout keyword exists."""
    return requests.get(url, **options)


def fetch_ignored_endpoint(url: str) -> requests.Response:
    """Suppressed exception for the request-timeout rule only."""
    # past: ignore request-timeout
    return requests.get(url)


class MemoryCache:
    def __init__(self) -> None:
        self._values: dict[str, Any] = {}

    def get(self, key: str) -> Any:
        """Compliant: this method is not requests.get."""
        return self._values.get(key)

    def save(self, key: str, value: Any) -> None:
        """Compliant: method name starts with a lowercase letter."""
        self._values[key] = value

    def Clear(self) -> None:
        """Violation: uppercase method under a class."""
        self._values.clear()

    def export_legacy(self) -> dict[str, Any]:
        """Compliant lowercase compatibility method."""
        return dict(self._values)


class LegacyAdapter:
    # past: ignore method-name-case
    def Export(self) -> dict[str, Any]:
        """Suppressed exception: retained for an external legacy API."""
        return {}


@dataclass
class ApiClient:
    base_url: str
    timeout: float = DEFAULT_TIMEOUT

    def build_url(self, path: str) -> str:
        """Compliant lowercase method."""
        return f"{self.base_url.rstrip('/')}/{path.lstrip('/')}"

    def load(self, path: str) -> requests.Response:
        """Compliant request and compliant method name."""
        return requests.get(self.build_url(path), timeout=self.timeout)

    def LoadWithoutTimeout(self, path: str) -> requests.Response:
        """Two violations: uppercase method and missing request timeout."""
        return requests.get(self.build_url(path))


def TopLevelFactory(base_url: str) -> ApiClient:
    """Compliant for method-name-case: this function is not inside a class."""
    return ApiClient(base_url)


class Parser:
    def parse(self, value: str) -> Any:
        """Compliant despite containing the substring 'eval' in a local name."""
        evaluated_default = {"value": value}
        return evaluated_default
