from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass
from typing import cast

from command import PublishError

_IDENTIFIER = r"[0-9A-Za-z-]+"
_VERSION_PATTERN = re.compile(
    rf"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    rf"(?:-({_IDENTIFIER}(?:\.{_IDENTIFIER})*))?"
    rf"(?:\+({_IDENTIFIER}(?:\.{_IDENTIFIER})*))?$"
)
_HEADING_PATTERN = re.compile(r"^\s*\[([^]]+)]\s*(?:#.*)?$")
_VERSION_FIELD_PATTERN = re.compile(r'(\bversion\s*=\s*")([^"]+)(")')
_PATH_FIELD_PATTERN = re.compile(r"\bpath\s*=")


@dataclass(frozen=True)
class Version:
    major: int
    minor: int
    patch: int
    prerelease: tuple[str, ...] | None
    build: tuple[str, ...] | None

    @property
    def core(self) -> tuple[int, int, int]:
        return self.major, self.minor, self.patch


@dataclass(frozen=True)
class ManifestUpdate:
    contents: str
    path_dependency_updates: int


def parse_version(version: str) -> Version:
    match = _VERSION_PATTERN.fullmatch(version)
    if match is None:
        raise PublishError(f"invalid version '{version}'")

    prerelease = tuple(match[4].split(".")) if match[4] else None
    if prerelease is not None:
        invalid_numeric = next(
            (
                identifier
                for identifier in prerelease
                if identifier.isdigit()
                and len(identifier) > 1
                and identifier.startswith("0")
            ),
            None,
        )
        if invalid_numeric is not None:
            raise PublishError(
                f"invalid version '{version}': numeric prerelease identifier "
                f"'{invalid_numeric}' has a leading zero"
            )

    build = tuple(match[5].split(".")) if match[5] else None
    return Version(
        major=int(match[1]),
        minor=int(match[2]),
        patch=int(match[3]),
        prerelease=prerelease,
        build=build,
    )


def _compare_versions(left: Version, right: Version) -> int:
    if left.core != right.core:
        return 1 if left.core > right.core else -1
    if left.prerelease is None:
        return 0 if right.prerelease is None else 1
    if right.prerelease is None:
        return -1

    for left_part, right_part in zip(left.prerelease, right.prerelease, strict=False):
        if left_part == right_part:
            continue

        left_numeric = left_part.isdigit()
        right_numeric = right_part.isdigit()
        if left_numeric and right_numeric:
            return 1 if int(left_part) > int(right_part) else -1
        if left_numeric != right_numeric:
            return -1 if left_numeric else 1
        return 1 if left_part > right_part else -1
    if len(left.prerelease) == len(right.prerelease):
        return 0
    return 1 if len(left.prerelease) > len(right.prerelease) else -1


def next_version(version: str) -> str:
    parsed = parse_version(version)
    if parsed.prerelease is None:
        return f"{parsed.major}.{parsed.minor}.{parsed.patch + 1}"

    prerelease = ".".join(parsed.prerelease)
    numbered = re.fullmatch(r"(.*?)(\d+)", prerelease)
    if numbered is None:
        prerelease = f"{prerelease}.1"
    else:
        prerelease = f"{numbered[1]}{int(numbered[2]) + 1}"
    return f"{parsed.major}.{parsed.minor}.{parsed.patch}-{prerelease}"


def require_version_advance(current_version: str, target_version: str) -> None:
    current = parse_version(current_version)
    target = parse_version(target_version)
    if _compare_versions(target, current) <= 0:
        raise PublishError(
            f"target version {target_version} is not newer than {current_version}"
        )


def workspace_version(cargo_manifest: str) -> str:
    try:
        parsed = tomllib.loads(cargo_manifest)
        version = parsed["workspace"]["package"]["version"]
    except (KeyError, TypeError, tomllib.TOMLDecodeError) as error:
        raise PublishError(
            "Cargo.toml does not define a valid workspace.package.version"
        ) from error
    if not isinstance(version, str):
        raise PublishError("Cargo.toml workspace.package.version must be a string")
    parse_version(version)
    return version


def update_cargo_manifest(
    cargo_manifest: str,
    current_version: str,
    target_version: str,
) -> ManifestUpdate:
    section = ""
    workspace_updates = 0
    path_dependency_updates = 0
    updated_lines: list[str] = []

    for line in cargo_manifest.splitlines(keepends=True):
        content = line.rstrip("\r\n")
        heading = _HEADING_PATTERN.fullmatch(content)
        if heading is not None:
            section = heading[1]
            updated_lines.append(line)
            continue

        version_field = _VERSION_FIELD_PATTERN.search(line)
        if version_field is None:
            updated_lines.append(line)
            continue

        is_workspace_version = section == "workspace.package"
        is_path_dependency = (
            section == "workspace.dependencies"
            and _PATH_FIELD_PATTERN.search(line) is not None
        )
        if not is_workspace_version and not is_path_dependency:
            updated_lines.append(line)
            continue
        if version_field[2] != current_version:
            raise PublishError(
                f"expected {current_version} in Cargo.toml, got {version_field[2]}"
            )

        if is_workspace_version:
            workspace_updates += 1
        if is_path_dependency:
            path_dependency_updates += 1
        updated_lines.append(
            f"{line[: version_field.start(2)]}{target_version}"
            f"{line[version_field.end(2) :]}"
        )

    if workspace_updates != 1:
        raise PublishError(
            f"expected one workspace version, updated {workspace_updates}"
        )
    return ManifestUpdate("".join(updated_lines), path_dependency_updates)


def update_json_version(
    document: dict[str, object],
    path: tuple[str, ...],
    current_version: str,
    target_version: str,
    *,
    label: str,
) -> None:
    if not path:
        raise PublishError(f"{label} version path is empty")

    container = document
    for key in path[:-1]:
        nested = container.get(key)
        if not isinstance(nested, dict):
            raise PublishError(
                f"expected {label} version {current_version}, got {nested}"
            )
        container = cast(dict[str, object], nested)

    field = path[-1]
    actual_version = container.get(field)
    if actual_version != current_version:
        raise PublishError(
            f"expected {label} version {current_version}, got {actual_version}"
        )
    container[field] = target_version


def parse_publish_remotes(configured_remotes: str) -> list[str]:
    remotes = configured_remotes.split()
    seen: set[str] = set()
    for remote in remotes:
        if remote in seen:
            raise PublishError(
                f"publishing remote '{remote}' is configured more than once"
            )
        seen.add(remote)
    return remotes


def order_publish_remotes(remotes: list[str], github_remote: str) -> list[str]:
    if github_remote not in remotes:
        raise PublishError(
            f"GitHub remote '{github_remote}' is not a publishing remote"
        )
    return [remote for remote in remotes if remote != github_remote] + [github_remote]


def push_command(remote: str, branch: str, tag: str) -> tuple[str, ...]:
    return (
        "git",
        "push",
        "--atomic",
        remote,
        f"HEAD:refs/heads/{branch}",
        f"refs/tags/{tag}:refs/tags/{tag}",
    )


def changed_paths(status: bytes) -> list[str]:
    if not status:
        return []
    if not status.endswith(b"\0"):
        raise PublishError("malformed Git status: missing NUL terminator")

    records = status[:-1].split(b"\0")
    paths: list[str] = []
    index = 0
    while index < len(records):
        record = records[index]
        if len(record) < 4 or record[2:3] != b" ":
            raise PublishError("malformed Git status entry")
        status_code = record[:2]
        paths.append(record[3:].decode("utf-8", errors="surrogateescape"))
        index += 1
        if b"R" in status_code or b"C" in status_code:
            if index >= len(records) or not records[index]:
                raise PublishError("malformed Git status rename entry")
            index += 1
    return paths
