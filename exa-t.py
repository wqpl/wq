#!/usr/bin/env python3
"""
exa-t.py

Usage examples:
  python3 exa-t.py gen                 # Generate .exp files
  python3 exa-t.py test <script>       # Run specified test
  python3 exa-t.py test --all, -a      # Run all tests
  python3 exa-t.py test --list, -l     # List all available tests
  python3 exa-t.py clean               # Remove all .exp files
"""

import subprocess
import sys
import os
import glob
import argparse


def check_environment():
    # Ensure script is run from project root (contains Cargo.toml)
    if not os.path.isfile("Cargo.toml"):
        print(
            "[ERROR] Cargo.toml not found. Please run this script from the Rust project root.",
            file=sys.stderr,
        )
        sys.exit(1)
    # Ensure exa/ directory exists
    if not os.path.isdir("exa"):
        print(
            "[ERROR] 'exa/' directory not found. Make sure you're in the correct folder and 'exa/' exists.",
            file=sys.stderr,
        )
        sys.exit(1)


def is_excluded(script_path):
    try:
        with open(script_path, "r", encoding="utf-8") as f:
            first_line = f.readline().lstrip()
            return first_line.startswith("// EXCLUDE")
    except OSError:
        return False


def generate_expected(build_type):
    check_environment()
    tests_dir = "exa"
    exp_dir = os.path.join(tests_dir, "exp")
    os.makedirs(exp_dir, exist_ok=True)

    all_tests = sorted(glob.glob(os.path.join(tests_dir, "*.wq")))
    included = [tf for tf in all_tests if not is_excluded(tf)]

    if not included:
        print("[ERROR] No test files to process in exa/ (*.wq)", file=sys.stderr)
        sys.exit(1)

    print(f"Building ({build_type}) before generating expectations...")
    run_cargo_build(build_type)

    for tf in included:
        basename = os.path.splitext(os.path.basename(tf))[0]
        out_path = os.path.join(exp_dir, f"{basename}.exp")
        print(f"Generating expected for {tf} → {out_path}")
        try:
            completed = subprocess.run(
                [f"./target/{build_type}/wq", tf],
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

        try:
            with open(out_path, "w", encoding="utf-8") as f:
                f.write(completed.stdout)
        except OSError as e:
            print(f"[ERROR] Could not write to '{out_path}': {e}", file=sys.stderr)
            sys.exit(1)

    print("All .exp files generated.")


def run_tests(build_type, script=None, run_all=False):
    check_environment()
    tests_dir = "exa"
    exp_dir = os.path.join(tests_dir, "exp")
    if not os.path.isdir(exp_dir):
        print(
            f"[ERROR] Expected directory '{exp_dir}' not found. Run 'gen' first.",
            file=sys.stderr,
        )
        sys.exit(1)

    all_tests = sorted(glob.glob(os.path.join(tests_dir, "*.wq")))
    included = [tf for tf in all_tests if not is_excluded(tf)]

    if not included:
        print("[ERROR] No test files to process in exa/ (*.wq)", file=sys.stderr)
        sys.exit(1)

    if script and run_all:
        print("[ERROR] Cannot specify a script name with --all.", file=sys.stderr)
        sys.exit(1)

    if script:
        included = [
            tf for tf in included if os.path.splitext(os.path.basename(tf))[0] == script
        ]
        if not included:
            print(f"[ERROR] No test named '{script}' found.", file=sys.stderr)
            sys.exit(1)
    elif not run_all:
        print(
            "[ERROR] Specify a script name or use --all to run all tests.",
            file=sys.stderr,
        )
        sys.exit(1)

    print(f"Building ({build_type}) before running tests...")
    run_cargo_build(build_type)

    total = len(included)
    passed = 0
    failed = 0

    for tf in included:
        basename = os.path.splitext(os.path.basename(tf))[0]
        exp_path = os.path.join(exp_dir, f"{basename}.exp")
        print(f"=== Running {tf} ===")
        try:
            completed = subprocess.run(
                [f"./target/{build_type}/wq", tf],
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

        actual = completed.stdout

        if not os.path.isfile(exp_path):
            print(
                f"[ERROR] Expected file '{exp_dir}/{basename}.exp' not found. Run 'gen' first.",
                file=sys.stderr,
            )
            sys.exit(1)
        try:
            with open(exp_path, "r", encoding="utf-8") as f:
                expected = f.read()
        except OSError as e:
            print(f"[ERROR] Could not read expected file: {e}", file=sys.stderr)
            sys.exit(1)

        if actual.strip() == expected.strip():
            print("pass")
            passed += 1
        else:
            print("fail")
            print("---- Expected ----")
            print(expected.rstrip())
            print("---- Actual ------")
            print(actual.rstrip())
            failed += 1
        print()

    print(f"SUMMARY: {passed}/{total} passed, {failed} failed.")
    sys.exit(failed)


def run_cargo_build(build_type):
    cmd = ["cargo", "build", "--quiet"]
    if build_type == "release":
        cmd.insert(2, "--release")
    try:
        subprocess.run(cmd, check=True)
    except subprocess.CalledProcessError as e:
        print(f"[ERROR] Failed to build {build_type}: {e}", file=sys.stderr)
        sys.exit(1)


def list_tests():
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


def clean_exp():
    exp_dir = os.path.join("exa", "exp")
    if os.path.isdir(exp_dir):
        files = glob.glob(os.path.join(exp_dir, "*.exp"))
        for f in files:
            try:
                os.remove(f)
            except OSError as e:
                print(f"[WARN] Failed to remove '{f}': {e}")
        print(f"Removed {len(files)} .exp files from {exp_dir}")
    else:
        print(f"No exp directory found at '{exp_dir}' to clean.")
    sys.exit(0)


def main():
    parser = argparse.ArgumentParser(
        description="Utility for generating expected outputs and running wq tests"
    )
    parser.add_argument(
        "--build-type",
        choices=["debug", "release"],
        help="Build type for cargo (debug or release).",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("gen", help="Generate .exp files from exa/*.wq")
    parser_test = subparsers.add_parser("test", help="Run or list tests")
    group = parser_test.add_mutually_exclusive_group(required=True)
    group.add_argument("script", nargs="?", help="Test script basename to run")
    group.add_argument("-a", "--all", action="store_true", help="Run all tests")
    group.add_argument(
        "-l", "--list", action="store_true", help="List all available tests"
    )
    subparsers.add_parser("clean", help="Remove all generated .exp files")

    args = parser.parse_args()

    # Always default to debug unless --build-type is explicitly provided
    build_type = args.build_type or "debug"

    if args.command == "gen":
        generate_expected(build_type)
    elif args.command == "test":
        if args.list:
            list_tests()
        else:
            run_tests(build_type, script=args.script, run_all=args.all)
    elif args.command == "clean":
        clean_exp()
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
