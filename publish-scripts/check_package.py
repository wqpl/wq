from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Any

from command import PublishError, run_command
from release_support import parse_version

SCRIPT_DIRECTORY = Path(__file__).resolve().parent
WORKSPACE_DIRECTORY = SCRIPT_DIRECTORY.parent
CRATE_DIRECTORY = WORKSPACE_DIRECTORY / "wq-wasm"


def _require_equal(label: str, actual: object, expected: object) -> None:
    if actual != expected:
        raise PublishError(f"{label}: expected {expected}, got {actual}")


def _read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise PublishError(f"could not read valid JSON from {path}: {error}") from error
    if not isinstance(value, dict):
        raise PublishError(f"expected a JSON object in {path}")
    return value


def validate_package_metadata(
    package_manifest: dict[str, Any],
    generated_manifest: dict[str, Any],
    cargo_metadata: dict[str, Any],
    release_tag: str | None = None,
) -> str:
    packages = cargo_metadata.get("packages")
    if not isinstance(packages, list):
        raise PublishError("Cargo metadata does not contain a package list")
    cargo_package = next(
        (
            item
            for item in packages
            if isinstance(item, dict) and item.get("name") == "wq-wasm"
        ),
        None,
    )
    if cargo_package is None:
        raise PublishError("Cargo metadata does not contain the wq-wasm package")

    _require_equal("npm package name", package_manifest.get("name"), "wq-wasm")
    package_version = package_manifest.get("version")
    if not isinstance(package_version, str):
        raise PublishError("npm package version must be a string")
    parsed_version = parse_version(package_version)
    _require_equal(
        "npm and Cargo versions",
        package_version,
        cargo_package.get("version"),
    )
    _require_equal(
        "generated package name",
        generated_manifest.get("name"),
        package_manifest["name"],
    )
    _require_equal(
        "generated package version",
        generated_manifest.get("version"),
        package_version,
    )
    repository = package_manifest.get("repository")
    repository_url = repository.get("url") if isinstance(repository, dict) else None
    _require_equal(
        "npm repository",
        repository_url,
        "git+https://github.com/wqpl/wq.git",
    )
    if release_tag:
        _require_equal("release tag", release_tag, f"v{package_version}")

    return "preview" if parsed_version.prerelease is not None else "latest"


def main() -> int:
    try:
        package_manifest = _read_json(CRATE_DIRECTORY / "package.json")
        generated_manifest = _read_json(CRATE_DIRECTORY / "pkg" / "package.json")
        metadata_output = run_command(
            (
                "cargo",
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--locked",
            ),
            cwd=WORKSPACE_DIRECTORY,
            capture=True,
        )
        try:
            cargo_metadata = json.loads(metadata_output)
        except json.JSONDecodeError as error:
            raise PublishError(
                f"cargo metadata returned invalid JSON: {error}"
            ) from error
        if not isinstance(cargo_metadata, dict):
            raise PublishError("cargo metadata did not return a JSON object")

        npm_tag = validate_package_metadata(
            package_manifest,
            generated_manifest,
            cargo_metadata,
            os.environ.get("RELEASE_TAG"),
        )
        package_name = package_manifest["name"]
        package_version = package_manifest["version"]
        print(f"{package_name}@{package_version} is ready for npm tag '{npm_tag}'")

        github_output = os.environ.get("GITHUB_OUTPUT")
        if github_output:
            with Path(github_output).open(
                "a", encoding="utf-8", newline="\n"
            ) as output:
                output.write(f"npm_tag={npm_tag}\n")
        return 0
    except (OSError, PublishError) as error:
        print(f"Package check failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
