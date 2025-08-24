#!/usr/bin/env python3
"""
exa-t.py

Workflow:
  gen  -> preview run: write outputs to exa/gen/<name>.out
  lock -> approve baseline: write lockfiles to exa/lock/<name>.lock
  test -> compare current run against exa/lock/*.lock

Notes:
  - Scripts whose first line starts with "// EXCLUDE" are skipped.

Examples:
  python3 exa-t.py gen                 # build + preview all tests to exa/gen/*.out
  python3 exa-t.py gen foo             # build + preview only exa/foo.wq -> exa/gen/foo.out
  python3 exa-t.py lock                # build + (re)create all lockfiles in exa/lock/*.lock
  python3 exa-t.py lock foo            # build + (re)create lockfile for foo only
  python3 exa-t.py test                # build + run all tests, compare to exa/lock/*.lock
  python3 exa-t.py test foo            # build + run foo only, compare to exa/lock/foo.lock
  python3 exa-t.py test --list, -l     # list available, non-excluded *.wq basenames
  python3 exa-t.py clean               # remove exa/gen/*.out

"""

import argparse
import glob
import os
import subprocess
import sys
from typing import List


# -------------------------
# Environment & utilities
# -------------------------


def check_environment() -> None:
    if not os.path.isfile("Cargo.toml"):
        print(
            "[ERROR] Cargo.toml not found. Please run this script from the Rust project root.",
            file=sys.stderr,
        )
        sys.exit(1)
    if not os.path.isdir("exa"):
        print(
            "[ERROR] 'exa/' directory not found. Make sure you're in the correct folder and 'exa/' exists.",
            file=sys.stderr,
        )
        sys.exit(1)


def run_cargo_build(build_type: str) -> None:
    cmd = ["cargo", "build", "--quiet"]
    if build_type == "release":
        cmd.insert(2, "--release")
    try:
        subprocess.run(cmd, check=True)
    except subprocess.CalledProcessError as e:
        print(f"[ERROR] Failed to build {build_type}: {e}", file=sys.stderr)
        sys.exit(1)


def is_excluded(script_path: str) -> bool:
    try:
        with open(script_path, "r", encoding="utf-8") as f:
            first_line = f.readline().lstrip()
            return first_line.startswith("// EXCLUDE")
    except OSError:
        return False


def collect_scripts(script: str | None) -> List[str]:
    tests_dir = "exa"
    all_tests = sorted(glob.glob(os.path.join(tests_dir, "*.wq")))
    included = [tf for tf in all_tests if not is_excluded(tf)]

    if not included:
        print("[ERROR] No test files to process in exa/ (*.wq)", file=sys.stderr)
        sys.exit(1)

    if script:
        base = os.path.splitext(os.path.basename(script))[0]
        included = [
            tf for tf in included if os.path.splitext(os.path.basename(tf))[0] == base
        ]
        if not included:
            print(f"[ERROR] No test named '{base}' found.", file=sys.stderr)
            sys.exit(1)

    return included


def run_wq(build_type: str, script_path: str) -> str:
    try:
        completed = subprocess.run(
            [f"./target/{build_type}/wq", script_path],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            encoding="utf-8",
            check=False,
        )
    except FileNotFoundError:
        print(
            "[ERROR] binary not found. Ensure Rust is installed, built, and in your PATH.",
            file=sys.stderr,
        )
        sys.exit(1)
    return completed.stdout


# -------------------------
# gen (preview) -> exa/gen/*.out
# -------------------------


def cmd_gen(build_type: str, script: str | None) -> None:
    check_environment()
    out_dir = os.path.join("exa", "gen")
    os.makedirs(out_dir, exist_ok=True)

    scripts = collect_scripts(script)

    print(f"Building ({build_type}) before preview generation...")
    run_cargo_build(build_type)

    for tf in scripts:
        basename = os.path.splitext(os.path.basename(tf))[0]
        out_path = os.path.join(out_dir, f"{basename}.out")
        print(f"[gen] {tf} -> {out_path}")
        actual = run_wq(build_type, tf)
        try:
            with open(out_path, "w", encoding="utf-8") as f:
                f.write(actual)
        except OSError as e:
            print(f"[ERROR] Could not write to '{out_path}': {e}", file=sys.stderr)
            sys.exit(1)

    print("Preview outputs written to exa/gen/*.out")


# -------------------------
# lock (approve) -> exa/lock/*.lock
# -------------------------


def cmd_lock(build_type: str, script: str | None) -> None:
    check_environment()
    lock_dir = os.path.join("exa", "lock")
    os.makedirs(lock_dir, exist_ok=True)

    # Clean lock folder first
    if script is None:
        old_files = glob.glob(os.path.join(lock_dir, "*.lock"))
        for f in old_files:
            try:
                os.remove(f)
            except OSError as e:
                print(f"[WARN] Failed to remove '{f}': {e}")
        if old_files:
            print(f"Cleaned {len(old_files)} old lockfiles from {lock_dir}")

    scripts = collect_scripts(script)

    print(f"Building ({build_type}) before locking...")
    run_cargo_build(build_type)

    for tf in scripts:
        basename = os.path.splitext(os.path.basename(tf))[0]
        lock_path = os.path.join(lock_dir, f"{basename}.lock")
        print(f"[lock] {tf} -> {lock_path}")
        actual = run_wq(build_type, tf)
        try:
            with open(lock_path, "w", encoding="utf-8") as f:
                f.write(actual)
        except OSError as e:
            print(f"[ERROR] Could not write to '{lock_path}': {e}", file=sys.stderr)
            sys.exit(1)

    print("Lockfiles written to exa/lock/*.lock")


# -------------------------
# test (compare) vs exa/lock/*.lock
# -------------------------


def cmd_test(build_type: str, script: str | None) -> None:
    check_environment()

    lock_dir = os.path.join("exa", "lock")
    if not os.path.isdir(lock_dir):
        print(
            f"[ERROR] Lock directory '{lock_dir}' not found. Run 'lock' first to create baselines.",
            file=sys.stderr,
        )
        sys.exit(1)

    scripts = collect_scripts(script)

    print(f"Building ({build_type}) before running tests...")
    run_cargo_build(build_type)

    total = len(scripts)
    passed = 0
    failed = 0

    for tf in scripts:
        basename = os.path.splitext(os.path.basename(tf))[0]
        lock_path = os.path.join(lock_dir, f"{basename}.lock")
        print(f"=== Running {tf} ===")

        if not os.path.isfile(lock_path):
            print(
                f"[ERROR] Missing lockfile '{lock_path}'. Run 'lock {basename}' to create it.",
                file=sys.stderr,
            )
            sys.exit(1)

        actual = run_wq(build_type, tf)

        try:
            with open(lock_path, "r", encoding="utf-8") as f:
                expected = f.read()
        except OSError as e:
            print(f"[ERROR] Could not read lockfile: {e}", file=sys.stderr)
            sys.exit(1)

        if actual.strip() == expected.strip():
            print("pass")
            passed += 1
        else:
            print("fail")
            print("---- Expected (lock) ----")
            print(expected.rstrip())
            print("---- Actual  (run)  ----")
            print(actual.rstrip())
            failed += 1
        print()

    print(f"SUMMARY: {passed}/{total} passed, {failed} failed.")
    sys.exit(failed)


# -------------------------
# list & clean
# -------------------------


def cmd_list() -> None:
    check_environment()
    tests_dir = "exa"
    all_tests = sorted(glob.glob(os.path.join(tests_dir, "*.wq")))
    included = [
        os.path.splitext(os.path.basename(tf))[0]
        for tf in all_tests
        if not is_excluded(tf)
    ]
    if not included:
        print("[INFO] No available tests found.")
    else:
        print("Available tests:")
        for name in included:
            print(f"  - {name}")
    sys.exit(0)


def cmd_clean() -> None:
    gen_dir = os.path.join("exa", "gen")
    if os.path.isdir(gen_dir):
        files = glob.glob(os.path.join(gen_dir, "*.out"))
        for f in files:
            try:
                os.remove(f)
            except OSError as e:
                print(f"[WARN] Failed to remove '{f}': {e}")
        print(f"Removed {len(files)} .out files from {gen_dir}")
    else:
        print(f"No gen directory found at '{gen_dir}' to clean.")
    sys.exit(0)


# -------------------------
# CLI
# -------------------------


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Utility for previewing (gen), locking, and testing wq scripts"
    )
    parser.add_argument(
        "--build-type",
        choices=["debug", "release"],
        help="Build type for cargo (debug or release).",
    )

    subparsers = parser.add_subparsers(dest="command", required=True)

    # gen
    p_gen = subparsers.add_parser("gen", help="Preview run to exa/gen/*.out")
    p_gen.add_argument("script", nargs="?", help="Optional script basename to preview")

    # lock
    p_lock = subparsers.add_parser("lock", help="Approve baseline to exa/lock/*.lock")
    p_lock.add_argument("script", nargs="?", help="Optional script basename to lock")

    # test
    p_test = subparsers.add_parser("test", help="Run tests against exa/lock/*.lock")
    p_test.add_argument("script", nargs="?", help="Optional script basename to test")
    p_test.add_argument(
        "-l", "--list", action="store_true", help="List available tests and exit"
    )

    # list (alias for test --list)
    subparsers.add_parser("list", help="List available tests")

    # clean
    subparsers.add_parser(
        "clean", help="Remove all generated preview files (exa/gen/*.out)"
    )

    args = parser.parse_args()

    build_type = args.build_type or "debug"

    if args.command == "gen":
        cmd_gen(build_type, args.script)
    elif args.command == "lock":
        cmd_lock(build_type, args.script)
    elif args.command == "test":
        if args.list:
            cmd_list()
        cmd_test(build_type, args.script)
    elif args.command == "list":
        cmd_list()
    elif args.command == "clean":
        cmd_clean()
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
