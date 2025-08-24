#!/usr/bin/env python3
"""
builtin-rcg.py

Usage examples:
  python3 builtin-rcg.py              # Generates a summary of builtin functions
  python3 builtin-rcg.py --fold 80
"""

import os
import re
import argparse
import textwrap

SRC_DIR = os.path.join("src")
BUILTINS_RS = os.path.join(SRC_DIR, "builtins.rs")
BUILTIN_DIR = os.path.join(SRC_DIR, "builtins")

ADD_RE = re.compile(r'self\.add\("([^"]+)",\s*([a-zA-Z0-9_:]+)\)')


def parse_builtins() -> list[tuple[str, str]]:
    builtins: list[tuple[str, str]] = []
    with open(BUILTINS_RS, "r", encoding="utf-8") as f:
        for line in f:
            m = ADD_RE.search(line)
            if m:
                name, path = m.groups()
                builtins.append((name, path))
    return builtins


def main():
    parser = argparse.ArgumentParser(description="Builtins refcard generator")
    parser.add_argument(
        "--fold", type=int, default=40, help="Maximum line width for folding the output"
    )
    args = parser.parse_args()

    builtins = parse_builtins()
    names = [name for name, _ in builtins]
    text = textwrap.fill(" ".join(names), width=args.fold)
    print(text)


if __name__ == "__main__":
    main()
