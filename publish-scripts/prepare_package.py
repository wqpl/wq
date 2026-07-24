from __future__ import annotations

import shutil
import sys
from pathlib import Path

SCRIPT_DIRECTORY = Path(__file__).resolve().parent
CRATE_DIRECTORY = SCRIPT_DIRECTORY.parent / "wq-wasm"


def main() -> int:
    source = CRATE_DIRECTORY / ".npmignore"
    package_directory = CRATE_DIRECTORY / "pkg"
    destination = package_directory / ".npmignore"
    try:
        if not source.is_file():
            raise FileNotFoundError(f"package ignore file does not exist: {source}")
        if not package_directory.is_dir():
            raise FileNotFoundError(
                f"wasm-pack output directory does not exist: {package_directory}"
            )
        shutil.copyfile(source, destination)
        return 0
    except OSError as error:
        print(f"Package preparation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
