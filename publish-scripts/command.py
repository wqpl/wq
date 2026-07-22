from __future__ import annotations

import os
import shlex
import subprocess
from collections.abc import Mapping, Sequence
from pathlib import Path


class PublishError(RuntimeError):
    """A publishing failure that can be reported without a traceback."""


def display_command(command: Sequence[str]) -> str:
    return shlex.join(command)


def run_command(
    command: Sequence[str],
    *,
    cwd: Path,
    env: Mapping[str, str] | None = None,
    capture: bool = False,
    announce: bool = True,
) -> str:
    arguments = tuple(command)
    if announce:
        print(f"\n$ {display_command(arguments)}", flush=True)

    process_env = os.environ.copy()
    if env is not None:
        process_env.update(env)

    try:
        result = subprocess.run(
            arguments,
            cwd=cwd,
            env=process_env,
            stdout=subprocess.PIPE if capture else None,
            text=True,
            check=False,
        )
    except OSError as error:
        raise PublishError(
            f"could not run '{arguments[0]}': {error.strerror or error}"
        ) from error

    if result.returncode < 0:
        raise PublishError(
            f"'{arguments[0]}' terminated by signal {-result.returncode}"
        )
    if result.returncode != 0:
        raise PublishError(f"'{arguments[0]}' exited with status {result.returncode}")
    return result.stdout.strip() if result.stdout is not None else ""


def run_bytes(
    command: Sequence[str],
    *,
    cwd: Path,
    acceptable_returncodes: frozenset[int] = frozenset({0}),
) -> tuple[int, bytes]:
    arguments = tuple(command)
    try:
        result = subprocess.run(
            arguments,
            cwd=cwd,
            stdout=subprocess.PIPE,
            check=False,
        )
    except OSError as error:
        raise PublishError(
            f"could not run '{arguments[0]}': {error.strerror or error}"
        ) from error

    if result.returncode not in acceptable_returncodes:
        if result.returncode < 0:
            detail = f"terminated by signal {-result.returncode}"
        else:
            detail = f"exited with status {result.returncode}"
        raise PublishError(f"'{arguments[0]}' {detail}")
    return result.returncode, result.stdout
