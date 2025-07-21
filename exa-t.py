#!/usr/bin/env python3
"""
exa-t.py

Usage:
  python3 exa-t.py gen   # Generate .exp files from exa/*.wq
  python3 exa-t.py test  # Compare outputs with .exp files
"""
import subprocess
import sys
import os
import glob
import argparse


def check_environment():
    # Ensure script is run from project root (contains Cargo.toml)
    if not os.path.isfile('Cargo.toml'):
        print("[ERROR] Cargo.toml not found. Please run this script from the Rust project root.", file=sys.stderr)
        sys.exit(1)
    # Ensure exa/ directory exists
    if not os.path.isdir('exa'):
        print("[ERROR] 'exa/' directory not found. Make sure you're in the right project and 'exa/' exists.", file=sys.stderr)
        sys.exit(1)


def trim_cargo_lines(output, num_lines=2):
    """
    Remove the first `num_lines` from the cargo output.
    Preserves trailing newline if present.
    """
    lines = output.splitlines()
    trimmed_lines = lines[num_lines:]
    trimmed = '\n'.join(trimmed_lines)
    if output.endswith('\n') and trimmed:
        trimmed += '\n'
    return trimmed


def generate_expected():
    check_environment()
    tests_dir = 'exa'
    exp_dir = os.path.join(tests_dir, 'exp')
    try:
        os.makedirs(exp_dir, exist_ok=True)
    except OSError as e:
        print(f"[ERROR] Failed to create exp directory '{exp_dir}': {e}", file=sys.stderr)
        sys.exit(1)

    test_files = sorted(glob.glob(os.path.join(tests_dir, '*.wq')))
    if not test_files:
        print("[ERROR] No test files found in exa/ (*.wq)", file=sys.stderr)
        sys.exit(1)

    for tf in test_files:
        basename = os.path.splitext(os.path.basename(tf))[0]
        out_path = os.path.join(exp_dir, f'{basename}.exp')
        print(f"Generating expected for {tf} → {out_path}")
        try:
            completed = subprocess.run(
                ['cargo', 'run', '--', tf],
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                encoding='utf-8',
                check=False
            )
        except FileNotFoundError:
            print("[ERROR] 'cargo' not found. Ensure Rust is installed and in your PATH.", file=sys.stderr)
            sys.exit(1)

        trimmed = trim_cargo_lines(completed.stdout, num_lines=2)

        try:
            with open(out_path, 'w', encoding='utf-8') as f:
                f.write(trimmed)
        except OSError as e:
            print(f"[ERROR] Could not write to '{out_path}': {e}", file=sys.stderr)
            sys.exit(1)

    print("All .exp files generated.")


def run_tests():
    check_environment()
    tests_dir = 'exa'
    exp_dir = os.path.join(tests_dir, 'exp')
    if not os.path.isdir(exp_dir):
        print(f"[ERROR] Expected directory '{exp_dir}' not found. Run 'gen' first.", file=sys.stderr)
        sys.exit(1)

    test_files = sorted(glob.glob(os.path.join(tests_dir, '*.wq')))
    if not test_files:
        print("[ERROR] No test files found in exa/ (*.wq)", file=sys.stderr)
        sys.exit(1)

    total = len(test_files)
    passed = 0
    failed = 0

    for tf in test_files:
        basename = os.path.splitext(os.path.basename(tf))[0]
        exp_path = os.path.join(exp_dir, f'{basename}.exp')
        print(f"=== Running {tf} ===")
        try:
            completed = subprocess.run(
                ['cargo', 'run', '--', tf],
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                encoding='utf-8',
                check=False
            )
        except FileNotFoundError:
            print("[ERROR] 'cargo' not found. Ensure Rust is installed and in your PATH.", file=sys.stderr)
            sys.exit(1)

        trimmed = trim_cargo_lines(completed.stdout, num_lines=2)

        if not os.path.isfile(exp_path):
            print(f"[ERROR] Expected file '{exp_dir}/{basename}.exp' not found.", file=sys.stderr)
            sys.exit(1)
        try:
            with open(exp_path, 'r', encoding='utf-8') as f:
                expected = f.read()
        except OSError as e:
            print(f"[ERROR] Could not read expected file '{exp_path}': {e}", file=sys.stderr)
            sys.exit(1)

        if trimmed.strip() == expected.strip():
            print("pass")
            passed += 1
        else:
            print("fail")
            print("---- Expected ----")
            print(expected.rstrip())
            print("---- Actual ------")
            print(trimmed.rstrip())
            failed += 1
        print()

    print(f"SUMMARY: {passed}/{total} passed, {failed} failed.")
    sys.exit(failed)


def main():
    parser = argparse.ArgumentParser(description='Utility for generating expected outputs and running tests')
    subparsers = parser.add_subparsers(dest='command', required=True)
    subparsers.add_parser('gen', help='Generate .exp files from exa/*.wq')
    subparsers.add_parser('test', help='Run tests')

    args = parser.parse_args()
    if args.command == 'gen':
        generate_expected()
    elif args.command == 'test':
        run_tests()
    else:
        parser.print_help()
        sys.exit(1)

if __name__ == '__main__':
    main()
