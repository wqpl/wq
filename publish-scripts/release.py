from __future__ import annotations

import argparse
import json
import os
import re
import stat
import sys
import tempfile
from pathlib import Path
from typing import Any

from command import PublishError, display_command, run_bytes, run_command
from release_support import (
    changed_paths,
    next_version,
    order_publish_remotes,
    parse_publish_remotes,
    parse_version,
    push_command,
    require_version_advance,
    update_cargo_manifest,
    workspace_version,
)


SCRIPT_DIRECTORY = Path(__file__).resolve().parent
WORKSPACE_DIRECTORY = SCRIPT_DIRECTORY.parent
CRATE_DIRECTORY = WORKSPACE_DIRECTORY / "wq-wasm"
CARGO_MANIFEST_PATH = WORKSPACE_DIRECTORY / "Cargo.toml"
CARGO_LOCK_PATH = WORKSPACE_DIRECTORY / "Cargo.lock"
PACKAGE_MANIFEST_PATH = CRATE_DIRECTORY / "package.json"
RELEASE_FILES = ("Cargo.toml", "Cargo.lock", "wq-wasm/package.json")
GITHUB_REMOTE_PATTERN = re.compile(r"github\.com[:/]wqpl/wq(?:\.git)?/?$")


def parse_arguments(arguments: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Update workspace versions, run release checks, create the release "
            "commit and tag, and optionally push them."
        ),
        epilog=(
            "examples:\n"
            "  python3 publish-scripts/release.py\n"
            "  python3 publish-scripts/release.py 0.10.0-preview1\n"
            "  python3 publish-scripts/release.py 0.10.0 --remote codeberg "
            "--remote github\n"
            "  python3 publish-scripts/release.py --no-push"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "version",
        nargs="?",
        help="target SemVer; defaults to the next preview or stable patch",
    )
    parser.add_argument(
        "--remote",
        action="append",
        default=[],
        metavar="NAME",
        help="publishing remote; repeat to override remotes.publish",
    )
    parser.add_argument(
        "--no-push",
        action="store_true",
        help="create the local commit and tag without offering to push",
    )
    return parser.parse_args(arguments)


def _read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        raise PublishError(f"could not read {path}: {error}") from error


def _read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(_read_text(path))
    except json.JSONDecodeError as error:
        raise PublishError(f"could not read valid JSON from {path}: {error}") from error
    if not isinstance(value, dict):
        raise PublishError(f"expected a JSON object in {path}")
    return value


def _write_text_atomic(path: Path, contents: str) -> None:
    temporary_path: Path | None = None
    try:
        mode = stat.S_IMODE(path.stat().st_mode)
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            newline="",
            prefix=f".{path.name}.",
            suffix=".publish-tmp",
            dir=path.parent,
            delete=False,
        ) as temporary:
            temporary.write(contents)
            temporary.flush()
            os.fsync(temporary.fileno())
            temporary_path = Path(temporary.name)
        temporary_path.chmod(mode)
        temporary_path.replace(path)
        temporary_path = None
    except OSError as error:
        raise PublishError(f"could not update {path}: {error}") from error
    finally:
        if temporary_path is not None:
            try:
                temporary_path.unlink(missing_ok=True)
            except OSError:
                pass


def _git_status_paths() -> list[str]:
    _, status = run_bytes(
        ("git", "status", "--porcelain=v1", "-z", "--untracked-files=all"),
        cwd=WORKSPACE_DIRECTORY,
    )
    return changed_paths(status)


def assert_clean_worktree() -> None:
    paths = _git_status_paths()
    if paths:
        details = "\n".join(f"  {path}" for path in paths)
        raise PublishError(f"worktree must be clean:\n{details}")


def configured_publish_remotes() -> list[str]:
    returncode, output = run_bytes(
        ("git", "config", "--get-all", "remotes.publish"),
        cwd=WORKSPACE_DIRECTORY,
        acceptable_returncodes=frozenset({0, 1}),
    )
    if returncode == 1:
        return []
    return parse_publish_remotes(output.decode("utf-8", errors="surrogateescape"))


def select_publish_remotes(overrides: list[str]) -> tuple[list[str], dict[str, str]]:
    remote_names = set(
        run_command(
            ("git", "remote"),
            cwd=WORKSPACE_DIRECTORY,
            capture=True,
            announce=False,
        ).splitlines()
    )
    if overrides:
        remotes = parse_publish_remotes("\n".join(overrides))
    else:
        remotes = configured_publish_remotes()
        if not remotes:
            remotes = ["github" if "github" in remote_names else "origin"]

    if not remotes:
        raise PublishError("at least one publishing remote is required")
    for remote in remotes:
        if remote not in remote_names:
            raise PublishError(f"git remote '{remote}' does not exist")

    remote_urls = {
        remote: run_command(
            ("git", "remote", "get-url", "--push", remote),
            cwd=WORKSPACE_DIRECTORY,
            capture=True,
            announce=False,
        )
        for remote in remotes
    }
    github_remotes = [
        remote
        for remote, url in remote_urls.items()
        if GITHUB_REMOTE_PATTERN.search(url) is not None
    ]
    if len(github_remotes) != 1:
        raise PublishError(
            "expected one publishing remote for the GitHub wq repository, "
            f"got {len(github_remotes)}"
        )
    return order_publish_remotes(remotes, github_remotes[0]), remote_urls


def assert_tag_absent(tag: str) -> None:
    returncode, _ = run_bytes(
        ("git", "show-ref", "--verify", "--quiet", f"refs/tags/{tag}"),
        cwd=WORKSPACE_DIRECTORY,
        acceptable_returncodes=frozenset({0, 1}),
    )
    if returncode == 0:
        raise PublishError(f"tag '{tag}' already exists")


def verify_workspace_versions(metadata: dict[str, Any], target_version: str) -> None:
    workspace_members = metadata.get("workspace_members")
    packages = metadata.get("packages")
    if not isinstance(workspace_members, list) or not isinstance(packages, list):
        raise PublishError("cargo metadata is missing workspace package data")

    member_ids = set(workspace_members)
    mismatches = [
        f"{item.get('name')}@{item.get('version')}"
        for item in packages
        if isinstance(item, dict)
        and item.get("id") in member_ids
        and item.get("version") != target_version
    ]
    if mismatches:
        raise PublishError(
            f"workspace versions did not update: {', '.join(mismatches)}"
        )


def verify_changed_files() -> None:
    changed = set(_git_status_paths())
    expected = set(RELEASE_FILES)
    unexpected = sorted(changed - expected)
    missing = sorted(expected - changed)
    if unexpected or missing:
        raise PublishError(
            "unexpected release changes; "
            f"extra: {', '.join(unexpected) or 'none'}; "
            f"missing: {', '.join(missing) or 'none'}"
        )


def run_release_checks(tag: str, target_version: str) -> None:
    metadata_output = run_command(
        ("cargo", "metadata", "--format-version", "1", "--offline"),
        cwd=WORKSPACE_DIRECTORY,
        capture=True,
    )
    try:
        metadata = json.loads(metadata_output)
    except json.JSONDecodeError as error:
        raise PublishError(f"cargo metadata returned invalid JSON: {error}") from error
    if not isinstance(metadata, dict):
        raise PublishError("cargo metadata did not return a JSON object")
    verify_workspace_versions(metadata, target_version)

    run_command(
        (
            sys.executable,
            "-m",
            "unittest",
            "discover",
            "-s",
            str(SCRIPT_DIRECTORY),
            "-t",
            str(SCRIPT_DIRECTORY),
            "-p",
            "test_*.py",
        ),
        cwd=WORKSPACE_DIRECTORY,
    )
    run_command(("cargo", "+nightly", "fmt", "--check"), cwd=WORKSPACE_DIRECTORY)
    run_command(
        ("cargo", "clippy", "--all-targets", "--", "-D", "warnings"),
        cwd=WORKSPACE_DIRECTORY,
    )
    run_command(("cargo", "test", "-p", "wq-wasm"), cwd=WORKSPACE_DIRECTORY)
    run_command(
        (
            "cargo",
            "publish",
            "--workspace",
            "--locked",
            "--dry-run",
            "--allow-dirty",
        ),
        cwd=WORKSPACE_DIRECTORY,
    )
    run_command(("npm", "run", "build"), cwd=CRATE_DIRECTORY)
    run_command(
        ("npm", "run", "check"),
        cwd=CRATE_DIRECTORY,
        env={"RELEASE_TAG": tag},
    )
    run_command(("npm", "test"), cwd=CRATE_DIRECTORY)
    run_command(("npm", "pack", "--dry-run"), cwd=CRATE_DIRECTORY)
    verify_changed_files()


def push_instructions(
    remotes: list[str], branch: str, tag: str
) -> list[tuple[str, ...]]:
    return [push_command(remote, branch, tag) for remote in remotes]


def offer_to_push(
    remotes: list[str],
    branch: str,
    tag: str,
    *,
    no_push: bool,
) -> None:
    commands = push_instructions(remotes, branch, tag)
    instructions = "\n".join(f"  {display_command(command)}" for command in commands)
    if no_push:
        print(f"\nPush skipped by --no-push. Publish later with:\n{instructions}")
        return
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        print(
            f"\nNot pushing from a non-interactive terminal.\nPublish later with:\n{instructions}"
        )
        return

    remote_list = ", ".join(repr(remote) for remote in remotes)
    try:
        answer = input(
            f"\nPush {tag} and {branch} to {remote_list} now? "
            "The GitHub tag push triggers package and GitHub Release publishing. "
            "[y/N] "
        )
    except EOFError:
        answer = ""
    if answer.strip().lower() not in {"y", "yes"}:
        print(f"Not pushed. Publish later with:\n{instructions}")
        return
    for command in commands:
        run_command(command, cwd=WORKSPACE_DIRECTORY)


def release(options: argparse.Namespace) -> None:
    assert_clean_worktree()
    branch = run_command(
        ("git", "branch", "--show-current"),
        cwd=WORKSPACE_DIRECTORY,
        capture=True,
        announce=False,
    )
    if not branch:
        raise PublishError("release requires a checked-out branch")

    ordered_remotes, remote_urls = select_publish_remotes(options.remote)
    original_cargo_manifest = _read_text(CARGO_MANIFEST_PATH)
    original_cargo_lock = _read_text(CARGO_LOCK_PATH)
    original_package_manifest = _read_text(PACKAGE_MANIFEST_PATH)
    package_manifest = _read_json(PACKAGE_MANIFEST_PATH)

    current_version = workspace_version(original_cargo_manifest)
    if package_manifest.get("version") != current_version:
        raise PublishError(
            f"Cargo version {current_version} does not match npm version "
            f"{package_manifest.get('version')}"
        )
    target_version = options.version or next_version(current_version)
    if target_version.startswith("v"):
        target_version = target_version[1:]
    parse_version(target_version)
    require_version_advance(current_version, target_version)
    tag = f"v{target_version}"
    assert_tag_absent(tag)

    print(f"Preparing {tag} from {branch}. Publishing remotes in push order:")
    for remote in ordered_remotes:
        print(f"  {remote}: {remote_urls[remote]}")

    manifest_update = update_cargo_manifest(
        original_cargo_manifest,
        current_version,
        target_version,
    )
    if manifest_update.path_dependency_updates == 0:
        raise PublishError("no versioned workspace path dependencies were updated")
    package_manifest["version"] = target_version

    _write_text_atomic(CARGO_MANIFEST_PATH, manifest_update.contents)
    _write_text_atomic(
        PACKAGE_MANIFEST_PATH,
        f"{json.dumps(package_manifest, indent=2, ensure_ascii=False)}\n",
    )

    git_mutation_started = False
    try:
        run_release_checks(tag, target_version)
        git_mutation_started = True
        run_command(("git", "add", *RELEASE_FILES), cwd=WORKSPACE_DIRECTORY)
        run_command(
            (
                "git",
                "commit",
                "-m",
                f"release {tag}",
                "-m",
                "Bump Cargo and npm package versions for the release.\n\n"
                "Release Notes:\n\n- N/A",
            ),
            cwd=WORKSPACE_DIRECTORY,
        )
        run_command(("git", "tag", "-a", tag, "-m", tag), cwd=WORKSPACE_DIRECTORY)
    except Exception:
        if git_mutation_started:
            print(
                "Release preparation reached Git staging. Version files were not "
                "restored; inspect the index, commit, and tag before retrying.",
                file=sys.stderr,
            )
        else:
            _write_text_atomic(CARGO_MANIFEST_PATH, original_cargo_manifest)
            _write_text_atomic(CARGO_LOCK_PATH, original_cargo_lock)
            _write_text_atomic(PACKAGE_MANIFEST_PATH, original_package_manifest)
            print(
                "Restored version files after the failed release check.",
                file=sys.stderr,
            )
        raise

    print(f"\nCreated release commit and local tag {tag}.")
    offer_to_push(
        ordered_remotes,
        branch,
        tag,
        no_push=options.no_push,
    )


def main(arguments: list[str] | None = None) -> int:
    try:
        options = parse_arguments(sys.argv[1:] if arguments is None else arguments)
        release(options)
        return 0
    except KeyboardInterrupt:
        print("\nRelease interrupted.", file=sys.stderr)
        return 130
    except Exception as error:
        print(f"\nRelease failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
