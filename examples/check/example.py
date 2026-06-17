"""Build a small release-readiness report for packages published on PyPI.

The normal path is useful: it queries package metadata, records the current Git
revision, optionally hashes a local distribution artifact, and prints JSON.

The module also contains a few believable legacy shortcuts for
``examples/check/pyastq.toml`` to find. They are intentionally retained so the
file can demonstrate structural matching, import alias resolution, descendant
chains, argument predicates, negation, query variables, captures, and targeted
suppressions without automated replacements.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import logging
import subprocess
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

import requests as http


PYPI_URL = "https://pypi.org/pypi/{package}/json"
DEFAULT_TIMEOUT = 10.0
logger = logging.getLogger("release-report")


@dataclass
class PackageStatus:
    name: str
    latest_version: str
    requires_python: str | None
    project_url: str | None


class PackageInspector:
    """Fetch package metadata from PyPI."""

    def __init__(self, timeout: float = DEFAULT_TIMEOUT) -> None:
        self.timeout = timeout

    def load_package(self, package: str) -> PackageStatus:
        """Clean: the request has a timeout and the method name is lowercase."""
        response = http.get(PYPI_URL.format(package=package), timeout=self.timeout)
        response.raise_for_status()
        metadata = response.json()["info"]
        return PackageStatus(
            name=metadata["name"],
            latest_version=metadata["version"],
            requires_python=metadata.get("requires_python"),
            project_url=metadata.get("project_url"),
        )

    def LoadPackage(self, package: str) -> PackageStatus:
        """Caught: uppercase method, missing timeout, and legacy descendant chain."""
        response = http.get(PYPI_URL.format(package=package))
        response.raise_for_status()
        metadata = response.json()["info"]
        return PackageStatus(
            name=metadata["name"],
            latest_version=metadata["version"],
            requires_python=metadata.get("requires_python"),
            project_url=metadata.get("project_url"),
        )


def current_revision() -> str:
    """Clean: the Git command checks its exit status and does not use a shell."""
    result = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def legacy_revision() -> str:
    """Caught: this Git command does not use check=True."""
    result = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"],
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def artifact_sha256(path: Path) -> str:
    """Clean: calculate a release artifact checksum with SHA-256."""
    return hashlib.sha256(path.read_bytes()).hexdigest()


def legacy_artifact_id(path: Path) -> str:
    """Caught: compatibility identifier using the obsolete MD5 digest."""
    return hashlib.md5(path.read_bytes()).hexdigest()


def suppressed_partner_artifact_id(path: Path) -> str:
    """Not reported: a targeted suppression documents a required exception."""
    return hashlib.md5(path.read_bytes()).hexdigest()  # pyastq: ignore weak-release-digest


def versions_match(current: str, expected: str) -> bool:
    """Return whether two normalized version strings are equal."""
    return current.strip().lower() == expected.strip().lower()


def compare_release_version(current_version: str, expected_version: str) -> bool:
    """Clean: compare the current release version with the expected version."""
    return versions_match(current_version, expected_version)


def compare_legacy_version(current_version: str) -> bool:
    """Caught: the same value appears twice and satisfies a repeated capture."""
    return versions_match(current_version, current_version)


def parse_labels(values: list[str]) -> dict[str, str]:
    """Parse repeated KEY=VALUE labels from the command line."""
    labels: dict[str, str] = {}
    for value in values:
        key, separator, label = value.partition("=")
        if not separator or not key:
            raise ValueError(f"invalid label {value!r}; expected KEY=VALUE")
        labels[key] = label
    return labels


def parse_legacy_overrides(expression: str) -> dict[str, Any]:
    """Caught: parse the obsolete Python-expression override format."""
    overrides = eval(expression)
    if not isinstance(overrides, dict):
        raise ValueError("legacy overrides must evaluate to a dictionary")
    return overrides


def parse_json_overrides(document: str) -> dict[str, Any]:
    """Clean: parse overrides as constrained JSON instead of evaluating code."""
    overrides = json.loads(document)
    if not isinstance(overrides, dict):
        raise ValueError("JSON overrides must contain an object")
    return overrides


def run_legacy_hook(command: str) -> None:
    """Caught: run an old user-provided release hook through a shell."""
    subprocess.run(command, shell=True, check=True)


def run_hook(command: list[str]) -> None:
    """Clean: execute a release hook as an argument list and check its status."""
    subprocess.run(command, check=True)


def build_report(
    packages: list[str],
    artifact: Path | None,
    labels: dict[str, str],
    use_legacy_client: bool = False,
) -> dict[str, Any]:
    inspector = PackageInspector()
    load = inspector.LoadPackage if use_legacy_client else inspector.load_package
    package_statuses = [asdict(load(package)) for package in packages]
    report: dict[str, Any] = {
        "revision": current_revision(),
        "packages": package_statuses,
        "labels": labels,
    }
    if artifact is not None:
        report["artifact"] = {
            "path": str(artifact),
            "sha256": artifact_sha256(artifact),
        }
    return report


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Create a JSON release-readiness report using PyPI metadata."
    )
    parser.add_argument("packages", nargs="+", help="PyPI package names to inspect")
    parser.add_argument("--artifact", type=Path, help="Distribution artifact to hash")
    parser.add_argument("--label", action="append", default=[], metavar="KEY=VALUE")
    parser.add_argument("--output", type=Path, help="Write JSON to this path")
    parser.add_argument("--overrides", help="JSON object merged into report labels")
    parser.add_argument("--legacy-client", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--legacy-revision", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--legacy-hook", help=argparse.SUPPRESS)
    parser.add_argument("--legacy-overrides", help=argparse.SUPPRESS)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    labels = parse_labels(args.label)
    if args.overrides:
        labels.update(parse_json_overrides(args.overrides))
    if args.legacy_overrides:
        labels.update(parse_legacy_overrides(args.legacy_overrides))

    report = build_report(args.packages, args.artifact, labels, args.legacy_client)
    if args.legacy_revision:
        report["revision"] = legacy_revision()
    rendered = json.dumps(report, indent=2, sort_keys=True)
    if args.output:
        args.output.write_text(rendered + "\n", encoding="utf-8")
        logger.info("wrote release report to %s", args.output)
    else:
        print(rendered)

    if args.legacy_hook:
        run_legacy_hook(args.legacy_hook)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
