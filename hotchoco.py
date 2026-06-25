#!/usr/bin/env python3
"""wq integration snapshot test suite

Usage:
    python hotchoco.py run [--no-build] [--group G] [--test T[/MODE]|G/T/MODE]
    python hotchoco.py diff [--group G] [--test T[/MODE]|G/T/MODE]
    python hotchoco.py review [--group G] [--test T]
    python hotchoco.py accept --all|--group G|--test T[/MODE]|G/T/MODE
    python hotchoco.py status [--verbose]
    python hotchoco.py show [--no-pager] [--group G] [--test T]
    python hotchoco.py clean

Testcase TOML may set expected_exit_code at the group or mode level. Use
[expected_exit_codes] with "test" or "test/mode" keys for individual overrides.
"""

from __future__ import annotations

import argparse
import difflib
import glob
import json
import os
import re
import shutil
import subprocess
import sys
from datetime import datetime
from pathlib import Path

import tomllib

SUITE_DIR = Path(__file__).resolve().parent / "hotchoco" / "suite"
PROJECT_ROOT = SUITE_DIR.parent.parent  # hotchoco/suite -> project root
DEFAULT_TIMEOUT_SECONDS = 30
DEFAULT_DIFF_CONTEXT_LINES = 3
WQ_CLI_DEBUG_BUILD_CMD = ["cargo", "build", "-p", "wq-cli"]
DEFAULT_EXPECTED_EXIT_CODE = 0


def resolve_glob(pattern: str) -> list[str]:
    """Resolve glob patterns relative to PROJECT_ROOT, return absolute paths."""
    cwd = os.getcwd()
    os.chdir(PROJECT_ROOT)
    try:
        return [str((PROJECT_ROOT / p).resolve()) for p in glob.glob(pattern)]
    finally:
        os.chdir(cwd)


# ── Config ──────────────────────────────────────────────────────────────────


def load_config() -> dict:
    with open(SUITE_DIR / "config.toml", "rb") as f:
        return tomllib.load(f)


def load_testcase(path: Path) -> dict:
    with open(path, "rb") as f:
        return tomllib.load(f)


def expected_exit_code_for(testcase: dict, mode: dict, test_name: str) -> int:
    overrides = testcase.get("expected_exit_codes", {})
    mode_name = mode["name"]
    for key in (f"{test_name}/{mode_name}", test_name):
        if key in overrides:
            return int(overrides[key])
    if "expected_exit_code" in mode:
        return int(mode["expected_exit_code"])
    if "expected_exit_code" in testcase:
        return int(testcase["expected_exit_code"])
    return DEFAULT_EXPECTED_EXIT_CODE


# ── Build ───────────────────────────────────────────────────────────────────


def build_wq_cli() -> None:
    sys.stderr.write("Building wq-cli debug binary...\n")
    sys.stderr.flush()
    proc = subprocess.run(WQ_CLI_DEBUG_BUILD_CMD, cwd=PROJECT_ROOT)
    if proc.returncode != 0:
        sys.exit(proc.returncode)


# ── ANSI strip ──────────────────────────────────────────────────────────────

ANSI_RE = re.compile(r"\x1b\[[0-9;]*[a-zA-Z]")


def strip_ansi(text: str) -> str:
    return ANSI_RE.sub("", text)


# ── Filters ─────────────────────────────────────────────────────────────────


def apply_filters(text: str, filters: list[dict]) -> str:
    for f in filters:
        text = re.sub(f["pattern"], f["replacement"], text)
    return text


def apply_filters_to_output(text: str, config: dict) -> str:
    if config.get("strip_ansi", True):
        text = strip_ansi(text)
    for f in config.get("filters", []):
        text = re.sub(f["pattern"], f["replacement"], text)
    return text


# ── Test discovery ──────────────────────────────────────────────────────────


def discover_testcases(config: dict) -> list[dict]:
    """Load all TOML testcase files and expand source_globs into individual tests."""
    result = []
    tc_dir = SUITE_DIR / "testcases"
    for tc_file in sorted(tc_dir.glob("*.toml")):
        tc = load_testcase(tc_file)
        group = tc["name"]
        sources = sorted(resolve_glob(tc["source_glob"]))
        exclude_marker = tc.get("exclude_marker", "//exclude")
        for src in sources:
            src_path = Path(src)
            # Check exclusion
            try:
                first_line = src_path.read_text().split("\n", 1)[0].strip().lower()
                if first_line.startswith(exclude_marker.lower()):
                    continue
            except Exception:
                pass
            test_name = src_path.stem
            output_extension = tc.get("output_extension", "")
            for mode in tc["modes"]:
                result.append(
                    {
                        "group": group,
                        "test": test_name,
                        "mode": mode["name"],
                        "source": str(src_path),
                        "subcommand": mode.get("subcommand"),
                        "flags": mode["flags"],
                        "capture": mode.get("capture", "stdout+stderr"),
                        "output_extension": output_extension,
                        "expected_exit_code": expected_exit_code_for(
                            tc,
                            mode,
                            test_name,
                        ),
                    }
                )
    return result


# ── Test execution ──────────────────────────────────────────────────────────


def run_one_test(test: dict, config: dict, output_dir: Path) -> dict:
    """Run one test and return {status, stdout, stderr, output}."""
    wq_bin_rel = config.get("wq_binary", "target/debug/wq")
    wq_bin = PROJECT_ROOT / wq_bin_rel
    timeout = config.get("timeout_seconds", DEFAULT_TIMEOUT_SECONDS)
    expected_exit_code = int(
        test.get("expected_exit_code", DEFAULT_EXPECTED_EXIT_CODE)
    )

    subcommand = test.get("subcommand")
    if subcommand:
        cmd = [str(wq_bin.resolve()), subcommand, test["source"]] + test["flags"]
    else:
        cmd = [str(wq_bin.resolve()), test["source"]] + test["flags"]
    try:
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=PROJECT_ROOT,
        )
        stdout = proc.stdout
        stderr = proc.stderr
        return_code = proc.returncode
    except subprocess.TimeoutExpired:
        stdout = ""
        stderr = f"TIMEOUT after {timeout}s"
        return_code = -1
    except Exception as e:
        stdout = ""
        stderr = str(e)
        return_code = -1

    # Capture based on mode config
    capture = test["capture"]
    if capture == "stdout":
        raw = stdout
    elif capture == "stderr":
        raw = stderr
    else:
        raw = stdout + stderr

    processed = apply_filters_to_output(raw, config)

    validation_errors = []
    # For fmt tests, run stabilisation check
    if subcommand == "fmt" and return_code == 0:
        import os as _os
        import tempfile as _tempfile

        with _tempfile.NamedTemporaryFile(mode="w", suffix=".wq", delete=False) as f:
            f.write(stdout)
            tmp_path = f.name

        try:
            # Stabilisation check
            stable_cmd = [str(wq_bin.resolve()), "fmt", tmp_path] + test["flags"]
            stable_proc = subprocess.run(
                stable_cmd,
                capture_output=True,
                text=True,
                timeout=timeout,
                cwd=PROJECT_ROOT,
            )
            if stable_proc.stdout != stdout:
                validation_errors.append(
                    "STABILISATION FAILED: re-formatting produced different output"
                )

        finally:
            _os.unlink(tmp_path)

    if validation_errors:
        processed += "\n\n=== VALIDATION ERRORS ===\n" + "\n".join(validation_errors)

    # Write output
    ext = test.get("output_extension", "")
    out_path = output_dir / test["group"] / test["test"] / f"{test['mode']}{ext}"
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(processed)

    return {
        "group": test["group"],
        "test": test["test"],
        "mode": test["mode"],
        "return_code": return_code,
        "expected_exit_code": expected_exit_code,
        "output": processed,
        "output_path": str(out_path),
    }


def run_tests(tests: list[dict], config: dict) -> tuple[Path, dict[str, dict]]:
    """Run all tests, return (output_dir, summary dict keyed by 'group/test/mode')."""
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    output_dir = (SUITE_DIR / "output" / ts).resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    total = len(tests)
    summary = {}
    for i, test in enumerate(tests):
        key = f"{test['group']}/{test['test']}/{test['mode']}"
        sys.stderr.write(f"\r[{i + 1}/{total}] {key} ... ")
        sys.stderr.flush()
        result = run_one_test(test, config, output_dir)
        # Compare with expected
        ext = test.get("output_extension", "")
        expected_path = (
            SUITE_DIR / "golden" / test["group"] / test["test"] / f"{test['mode']}{ext}"
        )
        if expected_path.exists():
            expected = expected_path.read_text()
            actual = result["output"]
            exit_ok = result["return_code"] == result["expected_exit_code"]
            passed = expected == actual and exit_ok
            summary[key] = {
                "status": "pass" if passed else "fail",
                "output_path": result["output_path"],
                "expected_path": str(expected_path),
                "return_code": result["return_code"],
                "expected_exit_code": result["expected_exit_code"],
            }
        else:
            exit_ok = result["return_code"] == result["expected_exit_code"]
            summary[key] = {
                "status": "new" if exit_ok else "fail",
                "output_path": result["output_path"],
                "expected_path": str(expected_path),
                "return_code": result["return_code"],
                "expected_exit_code": result["expected_exit_code"],
            }

    sys.stderr.write("\n")
    # Write summary
    summary_path = output_dir / "summary.json"
    summary_path.write_text(json.dumps(summary, indent=2))
    return output_dir, summary


# ── Diff ────────────────────────────────────────────────────────────────────


def compute_diff(expected: str, actual: str, label: str) -> str:
    lines_expected = diff_lines(expected)
    lines_actual = diff_lines(actual)
    matcher = difflib.SequenceMatcher(None, lines_expected, lines_actual)
    groups = list(matcher.get_grouped_opcodes(DEFAULT_DIFF_CONTEXT_LINES))
    if not groups:
        return ""

    result = [f"\033[1mgolden: {label}\033[0m", f"\033[1mactual: {label}\033[0m"]
    for group in groups:
        old_start = group[0][1] + 1
        old_end = group[-1][2]
        new_start = group[0][3] + 1
        new_end = group[-1][4]
        result.append(
            f"\033[36m@@ golden {old_start}-{old_end} / "
            f"actual {new_start}-{new_end} @@\033[0m"
        )

        for tag, i1, i2, j1, j2 in group:
            if tag == "equal":
                for line_no, line in enumerate(lines_expected[i1:i2], start=i1 + 1):
                    result.append(format_diff_line("=", line_no, line))
            if tag in ("replace", "delete"):
                for line_no, line in enumerate(lines_expected[i1:i2], start=i1 + 1):
                    result.append(format_diff_line("-", line_no, line))
            if tag in ("replace", "insert"):
                for line_no, line in enumerate(lines_actual[j1:j2], start=j1 + 1):
                    result.append(format_diff_line("+", line_no, line))
    return "\n".join(result)


def diff_lines(text: str) -> list[str]:
    lines = text.splitlines()
    if text and not text.endswith("\n"):
        lines[-1] = f"{lines[-1]} [no trailing newline]"
    return lines


def format_diff_line(kind: str, line_no: int, line: str) -> str:
    rendered = f"{kind:>4} {line_no:>5} | {line}"
    if kind == "old":
        return f"\033[31m{rendered}\033[0m"
    if kind == "new":
        return f"\033[32m{rendered}\033[0m"
    return rendered


# ── Find latest run ─────────────────────────────────────────────────────────


def latest_output_dir() -> Path | None:
    output_base = SUITE_DIR / "output"
    if not output_base.exists():
        return None
    dirs = sorted(output_base.iterdir(), reverse=True)
    for d in dirs:
        if (d / "summary.json").exists():
            return d
    return None


def load_summary(output_dir: Path | None = None) -> tuple[Path, dict]:
    if output_dir is None:
        output_dir = latest_output_dir()
    if output_dir is None:
        print("No previous run found. Run 'python hotchoco.py run' first.")
        sys.exit(1)
    summary = json.loads((output_dir / "summary.json").read_text())
    return output_dir, summary


# ── Filter tests by selectors ───────────────────────────────────────────────


def filter_tests(
    tests: list[dict],
    group: str | None = None,
    test_sel: str | None = None,
) -> list[dict]:
    """Filter test definitions by group and/or test selector.

    Test selectors accept any id suffix that appears in output:
      - test
      - test/mode
      - group/test/mode
    """
    result = []
    for t in tests:
        if test_sel:
            parts = test_sel.split("/")
            if len(parts) == 3:
                selector_group, test_name, modes = parts
                mode_names = set(modes.split(","))
                if t["group"] != selector_group:
                    continue
            elif len(parts) == 2:
                test_name, modes = parts
                mode_names = set(modes.split(","))
            else:
                test_name = parts[0]
                mode_names = None
            if t["test"] != test_name or (mode_names and t["mode"] not in mode_names):
                continue
        if group and t["group"] != group:
            continue
        result.append(t)
    return result


def filter_summary(
    summary: dict,
    group: str | None = None,
    test_sel: str | None = None,
) -> dict:
    """Filter summary entries by group and/or test selector."""
    result = {}
    for key, val in summary.items():
        g, t, m = key.split("/")
        if test_sel:
            parts = test_sel.split("/")
            if len(parts) == 3:
                selector_group, test_name, modes = parts
                mode_names = set(modes.split(","))
                if g != selector_group:
                    continue
            elif len(parts) == 2:
                test_name, modes = parts
                mode_names = set(modes.split(","))
            else:
                test_name = parts[0]
                mode_names = None
            if t != test_name or (mode_names and m not in mode_names):
                continue
        if group and g != group:
            continue
        result[key] = val
    return result


def selector_label(group: str | None, test_sel: str | None) -> str:
    parts = []
    if group:
        parts.append(f"group '{group}'")
    if test_sel:
        parts.append(f"test '{test_sel}'")
    return " and ".join(parts) if parts else "selection"


def exit_code_note(result: dict) -> str | None:
    return_code = result.get("return_code")
    expected = result.get("expected_exit_code")
    if return_code is None or expected is None or return_code == expected:
        return None
    return f"exit code {return_code}, expected {expected}"


# ── CLI commands ────────────────────────────────────────────────────────────


def cmd_run(args: argparse.Namespace) -> None:
    config = load_config()
    tests = discover_testcases(config)
    tests = filter_tests(tests, args.group, args.test)
    if not tests:
        print(f"No tests matched {selector_label(args.group, args.test)}.")
        return

    if not args.no_build:
        build_wq_cli()

    output_dir, summary = run_tests(tests, config)

    # Print summary
    passed = sum(1 for v in summary.values() if v["status"] == "pass")
    failed = sum(1 for v in summary.values() if v["status"] == "fail")
    new = sum(1 for v in summary.values() if v["status"] == "new")
    total = len(summary)

    print(f"\nResults: {passed} passed, {failed} failed, {new} new, {total} total")
    print(f"Output: {output_dir}")

    # Show diffs for failures
    if failed > 0 or new > 0:
        print()
        for key, val in sorted(summary.items()):
            if val["status"] == "pass":
                continue
            prefix = "FAIL" if val["status"] == "fail" else "NEW "
            print(f"=== {prefix}: {key} ===")
            note = exit_code_note(val)
            if note:
                print(f"  {note}")
            expected_path = Path(val["expected_path"])
            actual_path = Path(val["output_path"])
            if expected_path.exists():
                expected = expected_path.read_text()
                actual = actual_path.read_text()
                print(compute_diff(expected, actual, key))
                print()
            else:
                print("  New test — no expected file yet.")
                print(f"  Actual output ({len(actual_path.read_text())} bytes)")
                print()

    # Cleanup unreferenced golden files when running all tests
    if args.group is None and args.test is None:
        referenced = set()
        for t in tests:
            ext = t.get("output_extension", "")
            p = SUITE_DIR / "golden" / t["group"] / t["test"] / f"{t['mode']}{ext}"
            referenced.add(str(p.resolve()))

        removed = []
        expected_dir = SUITE_DIR / "golden"
        if expected_dir.exists():
            for golden_file in expected_dir.rglob("*"):
                if (
                    golden_file.is_file()
                    and str(golden_file.resolve()) not in referenced
                ):
                    golden_file.unlink()
                    removed.append(golden_file.relative_to(expected_dir))

            # Remove empty directories
            for dir_path in sorted(expected_dir.rglob("*"), reverse=True):
                if dir_path.is_dir() and not any(dir_path.iterdir()):
                    dir_path.rmdir()

        if removed:
            print(f"\nCleaned {len(removed)} unreferenced golden file(s):")
            for r in removed:
                print(f"  - {r}")

    if failed > 0:
        print("Run 'python hotchoco.py review' to interactively accept/reject changes.")
        sys.exit(1)


def cmd_diff(args: argparse.Namespace) -> None:
    output_dir, summary = load_summary(None)
    summary = filter_summary(summary, args.group, args.test)
    if not summary:
        print(f"No tests matched {selector_label(args.group, args.test)}.")
        return

    changed = {k: v for k, v in summary.items() if v["status"] != "pass"}
    if not changed:
        print("All tests passed. No diffs to show.")
        return

    for key, val in sorted(changed.items()):
        prefix = "FAIL" if val["status"] == "fail" else "NEW "
        print(f"=== {prefix}: {key} ===")
        note = exit_code_note(val)
        if note:
            print(f"  {note}")
        expected_path = Path(val["expected_path"])
        actual_path = Path(val["output_path"])
        if expected_path.exists():
            expected = expected_path.read_text()
            actual = actual_path.read_text()
            print(compute_diff(expected, actual, key))
        else:
            print("  New test — no expected file yet.")
            actual = actual_path.read_text()
            if len(actual) <= 2000:
                print(actual)
            else:
                print(f"  ({len(actual)} bytes, use --verbose or review to see)")
        print()


def cmd_show(args: argparse.Namespace) -> None:
    output_dir, summary = load_summary(None)
    summary = filter_summary(summary, args.group, args.test)
    if not summary:
        print(f"No tests matched {selector_label(args.group, args.test)}.")
        return

    changed = [(k, v) for k, v in sorted(summary.items()) if v["status"] != "pass"]
    if not changed:
        print("All tests passed. Nothing to show.")
        return

    lines = []
    for key, val in changed:
        expected_path = Path(val["expected_path"])
        actual_path = Path(val["output_path"])

        prefix = "FAIL" if val["status"] == "fail" else "NEW "
        lines.append(f"=== {prefix}: {key} ===")
        note = exit_code_note(val)
        if note:
            lines.append(f"  {note}")

        if expected_path.exists():
            expected = expected_path.read_text()
            actual = actual_path.read_text()
            lines.append(compute_diff(expected, actual, key))
        else:
            lines.append("  New test — no expected file yet.")
            actual = actual_path.read_text()
            for line in actual.splitlines():
                lines.append(f"  > {line}")
        lines.append("")

    output = "\n".join(lines)

    if args.no_pager:
        sys.stdout.write(output)
    else:
        pager = os.environ.get("PAGER", "less -R")
        if pager:
            try:
                subprocess.run(pager.split(), input=output, text=True)
            except FileNotFoundError:
                sys.stdout.write(output)
        else:
            sys.stdout.write(output)


def cmd_accept(args: argparse.Namespace) -> None:
    output_dir, full_summary = load_summary(None)
    summary = filter_summary(full_summary, args.group, args.test)

    if not args.all and not args.group and not args.test:
        print("Use --all, --group, or --test to specify what to accept.")
        print("Or use 'python hotchoco.py review' for interactive mode.")
        sys.exit(1)

    if not summary:
        print(f"No tests matched {selector_label(args.group, args.test)}.")
        sys.exit(1)

    accepted = 0
    for key, val in summary.items():
        if val["status"] == "pass":
            continue
        expected_path = Path(val["expected_path"])
        actual_path = Path(val["output_path"])
        expected_path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(actual_path, expected_path)
        full_summary[key]["status"] = "pass"
        print(f"  ACCEPT: {key}")
        accepted += 1

    if accepted:
        (output_dir / "summary.json").write_text(json.dumps(full_summary, indent=2))
        print(f"\nAccepted {accepted} change(s).")
    elif args.group or args.test:
        print(f"No changes to accept ({len(summary)} selected test(s) already pass).")
    else:
        print("No changes to accept (all tests pass).")


def cmd_status(args: argparse.Namespace) -> None:
    output_dir = latest_output_dir()
    if output_dir is None:
        print("No previous run. Run 'python hotchoco.py run' first.")
        return

    summary = json.loads((output_dir / "summary.json").read_text())
    summary = filter_summary(summary, args.group, args.test)
    if not summary:
        print(f"No tests matched {selector_label(args.group, args.test)}.")
        return

    passed = sum(1 for v in summary.values() if v["status"] == "pass")
    failed = sum(1 for v in summary.values() if v["status"] == "fail")
    new = sum(1 for v in summary.values() if v["status"] == "new")

    print(f"Run: {output_dir.name}")
    print(
        f"     {passed} passed, {failed} failed, {new} new, {passed + failed + new} total"
    )

    if args.verbose:
        print()
        for key, val in sorted(summary.items()):
            icon = {"pass": "✓", "fail": "✗", "new": "+"}[val["status"]]
            color = {"pass": "32", "fail": "31", "new": "33"}[val["status"]]
            note = exit_code_note(val)
            suffix = f" ({note})" if note else ""
            print(f"  \033[{color}m{icon}\033[0m {key}{suffix}")


def cmd_clean(args: argparse.Namespace) -> None:
    output_base = SUITE_DIR / "output"
    if not output_base.exists():
        print("No output directory found.")
        return

    # Only consider directories that contain summary.json
    dirs = sorted(
        [
            d
            for d in output_base.iterdir()
            if d.is_dir() and (d / "summary.json").exists()
        ]
    )
    if not dirs:
        print("No output runs to clean.")
        return

    latest = dirs[-1]
    removed = []
    for d in dirs[:-1]:
        shutil.rmtree(d)
        removed.append(d.name)

    if removed:
        print(f"Removed {len(removed)} old output run(s):")
        for name in removed:
            print(f"  - {name}")
        print(f"Kept: {latest.name}")
    else:
        print(f"Nothing to clean. Latest: {latest.name}")


def cmd_review(args: argparse.Namespace) -> None:
    output_dir, full_summary = load_summary(None)
    summary = filter_summary(full_summary, args.group, args.test)
    if not summary:
        print(f"No tests matched {selector_label(args.group, args.test)}.")
        return

    changed = [(k, v) for k, v in sorted(summary.items()) if v["status"] != "pass"]
    if not changed:
        print("All tests pass. Nothing to review.")
        return

    total = len(changed)
    idx = 0
    while idx < len(changed):
        key, val = changed[idx]
        expected_path = Path(val["expected_path"])
        actual_path = Path(val["output_path"])

        print(f"\n─── [{idx + 1}/{total}] {key} ", end="")
        if val["status"] == "new":
            print("\033[33m(NEW)\033[0m ", end="")
        print("─" * max(0, 60 - len(key)))
        note = exit_code_note(val)
        if note:
            print(f"  {note}")

        if expected_path.exists():
            expected = expected_path.read_text()
            actual = actual_path.read_text()
            print(compute_diff(expected, actual, key))
        else:
            print("\033[33m  (new test — no expected file)\033[0m")
            actual = actual_path.read_text()
            if len(actual) <= 500:
                for line in actual.splitlines():
                    print(f"  > {line}")
            else:
                preview = "\n".join(actual.splitlines()[:20])
                for line in preview.splitlines():
                    print(f"  > {line}")
                print(
                    f"  ... ({len(actual)} bytes total, {len(actual.splitlines())} lines)"
                )

        print()
        while True:
            try:
                resp = (
                    input("  [A]ccept  [S]kip  [V]iew  [Q]uit  [?]help: ")
                    .strip()
                    .lower()
                )
            except (EOFError, KeyboardInterrupt):
                resp = "q"

            if resp in ("a", "accept"):
                expected_path.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy(actual_path, expected_path)
                print("  ✓ Accepted")
                full_summary[key]["status"] = "pass"
                (output_dir / "summary.json").write_text(
                    json.dumps(full_summary, indent=2)
                )
                idx += 1
                break
            elif resp in ("s", "skip"):
                print("  → Skipped")
                idx += 1
                break
            elif resp in ("v", "view"):
                print(f"\n  ── expected ({expected_path}) ──")
                for i, line in enumerate(expected_path.read_text().splitlines()[:50]):
                    print(f"  {i + 1:4d}: {line}")
                if len(expected_path.read_text().splitlines()) > 50:
                    print(
                        f"  ... ({len(expected_path.read_text().splitlines())} lines total)"
                    )
                print(f"\n  ── actual ({actual_path}) ──")
                actual_full = actual_path.read_text()
                for i, line in enumerate(actual_full.splitlines()[:50]):
                    print(f"  {i + 1:4d}: {line}")
                if len(actual_full.splitlines()) > 50:
                    print(f"  ... ({len(actual_full.splitlines())} lines total)")
                print()
                continue
            elif resp in ("q", "quit"):
                remaining = total - idx
                # accepted = total - len(changed) + sum(
                #     1 for i in range(idx) if i < len(changed)
                # )
                print(f"\n  Quit. {idx} reviewed, {remaining} remaining.")
                return
            elif resp in ("?", "h", "help"):
                print()
                print("  A — accept change (copy actual → expected)")
                print("  S — skip to next")
                print("  V — view full expected and actual files")
                print("  Q — quit review")
                print("  ? — this help")
                print()
                continue
            else:
                print("  Unknown command. Type ? for help.")
                continue

    print(f"\nReview complete — all {total} changes reviewed.")


# ── CLI ─────────────────────────────────────────────────────────────────────


def main() -> None:
    parser = argparse.ArgumentParser(
        description="wq integration test suite (DIY snapshot testing)",
        prog="hotchoco.py",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    # run
    p_run = sub.add_parser("run", help="Run tests and compare against golden/")
    p_run.add_argument("--group", "-g", help="Test group to run")
    p_run.add_argument(
        "--test",
        "-t",
        help="Test to run (e.g. 'fib', 'fib/print', or 'wq/fib/exec')",
    )
    p_run.add_argument(
        "--no-build",
        action="store_true",
        help="Skip the default debug build before running tests",
    )

    # diff
    p_diff = sub.add_parser("diff", help="Show diffs from last run")
    p_diff.add_argument("--group", "-g", help="Filter by group")
    p_diff.add_argument(
        "--test",
        "-t",
        help="Filter by test (e.g. 'fib', 'fib/print', or 'wq/fib/exec')",
    )

    # show
    p_show = sub.add_parser("show", help="Dump diffs from last run to pager")
    p_show.add_argument("--no-pager", action="store_true", help="Print to stdout")
    p_show.add_argument("--group", "-g", help="Filter by group")
    p_show.add_argument(
        "--test",
        "-t",
        help="Filter by test (e.g. 'fib', 'fib/print', or 'wq/fib/exec')",
    )

    # review
    p_review = sub.add_parser("review", help="Interactive review of changed tests")
    p_review.add_argument("--group", "-g", help="Filter by group")
    p_review.add_argument(
        "--test",
        "-t",
        help="Filter by test (e.g. 'fib', 'fib/print', or 'wq/fib/exec')",
    )

    # accept
    p_accept = sub.add_parser("accept", help="Bulk accept changed tests")
    p_accept.add_argument("--all", action="store_true", help="Accept all changes")
    p_accept.add_argument("--group", "-g", help="Accept changes in one group")
    p_accept.add_argument(
        "--test",
        "-t",
        help="Accept one test (e.g. 'fib', 'fib/print', or 'wq/fib/exec')",
    )

    # status
    p_status = sub.add_parser("status", help="Show pass/fail summary from last run")
    p_status.add_argument(
        "--verbose", "-v", action="store_true", help="Per-test details"
    )
    p_status.add_argument("--group", "-g", help="Filter by group")
    p_status.add_argument(
        "--test",
        "-t",
        help="Filter by test (e.g. 'fib', 'fib/print', or 'wq/fib/exec')",
    )

    # clean
    sub.add_parser("clean", help="Remove all but the latest output directory")

    args = parser.parse_args()

    # Route to command
    match args.command:
        case "run":
            cmd_run(args)
        case "diff":
            cmd_diff(args)
        case "show":
            cmd_show(args)
        case "review":
            cmd_review(args)
        case "accept":
            cmd_accept(args)
        case "status":
            cmd_status(args)
        case "clean":
            cmd_clean(args)


if __name__ == "__main__":
    main()
