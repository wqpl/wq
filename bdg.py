"""
exa-t.py

Usage examples:
  python3 bdg.py concise

"""

import os
import re
import argparse
import textwrap

SRC_DIR = os.path.join("src")
BUILTINS_RS = os.path.join(SRC_DIR, "builtins.rs")
BUILTIN_DIR = os.path.join(SRC_DIR, "builtins")

ADD_RE = re.compile(r'self\.add\("([^"]+)",\s*([a-zA-Z0-9_:]+)\)')


def parse_builtins():
    builtins = []
    with open(BUILTINS_RS, "r", encoding="utf-8") as f:
        for line in f:
            m = ADD_RE.search(line)
            if m:
                name, path = m.groups()
                builtins.append((name, path))
    return builtins


EXPECT_RE = re.compile(r'"([^"\n]*?expects[^"\n]*)"')


def extract_expect_msg(lines, start):
    for i in range(start, len(lines)):
        if i > start and re.search(r"\bfn\s+\w+\s*\(", lines[i]):
            break
        if i > start and "bind_math!" in lines[i]:
            break
        m = EXPECT_RE.search(lines[i])
        if m:
            msg = m.group(1)
            m2 = re.search(r"expects\s+(.+?arguments?)", msg)
            if m2:
                return m2.group(1).strip()
            m2 = re.search(r"expects\s+(.+?argument)", msg)
            if m2:
                return m2.group(1).strip()
            m2 = re.search(r"expects\s+(.*)", msg)
            if m2:
                return m2.group(1).strip()
            return msg
    return None


def get_arg_info(module, func):
    path = os.path.join(BUILTIN_DIR, f"{module}.rs")
    if not os.path.isfile(path):
        return "var"
    with open(path, "r", encoding="utf-8") as f:
        lines = f.readlines()
        text = "".join(lines)

    # macro based math functions
    if re.search(rf"\bbind_math!\(\s*{func}\b", text):
        return "1 argument"

    # search for function definition
    pattern = re.compile(rf"fn\s+{func}\s*\(")
    for idx, line in enumerate(lines):
        if pattern.search(line):
            msg = extract_expect_msg(lines, idx)
            if msg:
                return msg
            break

    # fallback: search globally for "func expects"
    m = re.search(rf'"{func}\s*expects\s*([^"\n]+)"', text)
    if m:
        return m.group(1).strip()

    return "var"


def main():
    parser = argparse.ArgumentParser(description="Builtin doc generator")
    parser.add_argument("mode", choices=["concise", "detailed"], help="Output mode")
    args = parser.parse_args()

    builtins = parse_builtins()

    if args.mode == "concise":
        names = [name for name, _ in builtins]
        text = textwrap.fill(" ".join(names), width=40)
        print(text)
    else:
        rows = []
        for name, path in builtins:
            parts = path.split("::")
            module = parts[0]
            func = parts[-1]
            arg_info = get_arg_info(module, func)
            rows.append(f"{name}: {arg_info}")
        for row in rows:
            print(row)


if __name__ == "__main__":
    main()
