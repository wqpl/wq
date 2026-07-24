from __future__ import annotations

import argparse
import subprocess
import sys
from collections.abc import Sequence
from pathlib import Path


class ReleaseNotesError(RuntimeError):
    pass


def git_output(*arguments: str) -> str:
    try:
        return subprocess.check_output(
            ("git", *arguments),
            stderr=subprocess.PIPE,
            text=True,
        )
    except subprocess.CalledProcessError as error:
        detail = error.stderr.strip()
        if detail:
            raise ReleaseNotesError(detail) from error
        raise ReleaseNotesError(f"git {' '.join(arguments)} failed") from error


def git_output_bytes(*arguments: str) -> bytes:
    try:
        return subprocess.check_output(
            ("git", *arguments),
            stderr=subprocess.PIPE,
        )
    except subprocess.CalledProcessError as error:
        detail = error.stderr.decode(errors="replace").strip()
        if detail:
            raise ReleaseNotesError(detail) from error
        raise ReleaseNotesError(f"git {' '.join(arguments)} failed") from error


def previous_tag(tag: str) -> str | None:
    try:
        previous = git_output("describe", "--tags", "--abbrev=0", f"{tag}^")
    except ReleaseNotesError:
        return None
    return previous.strip()


def commit_messages(tag: str, previous: str | None) -> list[str]:
    revision = f"{previous}..{tag}" if previous is not None else tag
    output = git_output_bytes(
        "log",
        "--reverse",
        "--format=%B%x00",
        revision,
    )
    return [
        record.decode(errors="replace").strip()
        for record in output.split(b"\0")
        if record.strip()
    ]


def _release_note_blocks(message: str) -> list[str]:
    lines = message.splitlines()
    try:
        heading_index = lines.index("Release Notes:")
    except ValueError:
        return []

    blocks: list[str] = []
    current: list[str] = []
    for line in lines[heading_index + 1 :]:
        if line.startswith("- "):
            if current:
                blocks.append("\n".join(current))
            current = [line.rstrip()]
        elif current and line.startswith((" ", "\t")):
            current.append(line.rstrip())
        elif not line.strip():
            if current:
                blocks.append("\n".join(current))
                current = []
        elif blocks or current:
            break

    if current:
        blocks.append("\n".join(current))
    return blocks


def extract_release_notes(messages: Sequence[str]) -> list[str]:
    notes: list[str] = []
    for message in messages:
        notes.extend(
            block for block in _release_note_blocks(message) if block.strip() != "- N/A"
        )
    return notes


def render_release_notes(notes: Sequence[str]) -> str:
    if not notes:
        return "No user-facing changes were recorded.\n"
    return "\n\n".join(notes) + "\n"


def parse_arguments(arguments: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Extract commit-message Release Notes entries since the previous tag."
        )
    )
    parser.add_argument("tag", help="release tag whose reachable commits to inspect")
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="Markdown file to write",
    )
    return parser.parse_args(arguments)


def main(arguments: Sequence[str] | None = None) -> int:
    parsed = parse_arguments(arguments)
    try:
        previous = previous_tag(parsed.tag)
        notes = extract_release_notes(commit_messages(parsed.tag, previous))
    except ReleaseNotesError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    parsed.output.write_text(render_release_notes(notes), encoding="utf-8")
    start = previous if previous is not None else "the beginning of history"
    print(f"wrote {len(notes)} release note entries from {start} to {parsed.tag}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
