#!/usr/bin/env python3
"""
WQ Benchmark Runner

A hyperfine-based benchmarking script for the wq interpreter.

------------------------------------------------------------------------------
DEPENDENCIES
------------------------------------------------------------------------------
    pip install matplotlib   # for plots
    pip install rich         # optional prettier terminal output

    Also requires hyperfine and cargo to be installed:
        brew install hyperfine   # macOS

------------------------------------------------------------------------------
USAGE
------------------------------------------------------------------------------
    # Full benchmark run. Defaults to the repo's custom R profile.
    python3 wqbench.py

    # Benchmark another Cargo profile.
    python3 wqbench.py --profile dev
    python3 wqbench.py --profile release

    # Skip cargo build and use the binary for the selected profile.
    python3 wqbench.py --no-build --profile R

    # Use an explicit binary path.
    python3 wqbench.py --binary target/R/wq --no-build

    # Quick validation with fewer runs.
    python3 wqbench.py --no-build --min-runs 3 --warmup 1

    # Override version string (default: git tag > commit hash).
    python3 wqbench.py --version v1.2.0

    # Skip plots or persistence (useful in CI when you only want the table).
    python3 wqbench.py --no-plots --no-persist

    # Re-analyze existing history and (re)generate trend.png.
    python3 wqbench.py --analyze-only

------------------------------------------------------------------------------
DIRECTORY LAYOUT
------------------------------------------------------------------------------
    .benchmarks/
    |-- v1.0.0_R_20260101_120000/   one directory per run
    |   |-- benches/                per-benchmark hyperfine exports
    |   |-- plots/                  summary.png and trend.png
    |   `-- summary.csv             aggregated results for this run
    `-- history.jsonl               append-only regression history

------------------------------------------------------------------------------
REGRESSION ANALYSIS
------------------------------------------------------------------------------
    History is compared only within the same build profile. Each benchmark entry
    stores the source script SHA-256, so a same-name benchmark with different
    contents starts a fresh comparison series. Older records without embedded
    hashes are checked against their recorded git commit when possible.
    Scripts that exit nonzero are skipped and reported, rather than aborting the
    whole benchmark run.

    The table shows current timing, previous matching timing, delta, historical
    mean, sample count, and z-score status. Exit code is 1 if any benchmark is
    statistically slower with z-score > 2.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import re
import shlex
import statistics
import subprocess
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

try:
    from rich.console import Console
    from rich.panel import Panel
    from rich.table import Table
except ModuleNotFoundError:
    RICH_AVAILABLE = False

    def strip_markup(text: str) -> str:
        return re.sub(r"\[/?[A-Za-z][A-Za-z0-9 _#=./-]*\]", "", text)

    class Console:
        def print(self, *objects: object, sep: str = " ", end: str = "\n") -> None:
            print(*(strip_markup(str(obj)) for obj in objects), sep=sep, end=end)

    class Panel:
        def __init__(self, renderable: object) -> None:
            self.renderable = renderable

        def __str__(self) -> str:
            return strip_markup(str(self.renderable))

    class Table:
        def __init__(
            self,
            title: str = "",
            show_header: bool = True,
            header_style: str = "",
        ) -> None:
            self.title = strip_markup(title)
            self.show_header = show_header
            self.columns: list[dict[str, Any]] = []
            self.rows: list[list[str]] = []

        def add_column(self, header: str, **kwargs: Any) -> None:
            self.columns.append({"header": strip_markup(header), **kwargs})

        def add_row(self, *cells: object) -> None:
            self.rows.append([strip_markup(str(cell)) for cell in cells])

        def __str__(self) -> str:
            headers = [str(col["header"]) for col in self.columns]
            widths = [len(header) for header in headers]
            for row in self.rows:
                for idx, cell in enumerate(row):
                    widths[idx] = max(widths[idx], len(cell))

            def fmt_cell(text: str, idx: int) -> str:
                justify = self.columns[idx].get("justify")
                if justify == "right":
                    return text.rjust(widths[idx])
                if justify == "center":
                    return text.center(widths[idx])
                return text.ljust(widths[idx])

            lines: list[str] = []
            if self.title:
                lines.append(self.title)
            if self.show_header:
                lines.append(
                    "  ".join(
                        fmt_cell(header, idx) for idx, header in enumerate(headers)
                    )
                )
                lines.append("  ".join("-" * width for width in widths))
            for row in self.rows:
                lines.append(
                    "  ".join(fmt_cell(cell, idx) for idx, cell in enumerate(row))
                )
            return "\n".join(lines)
else:
    RICH_AVAILABLE = True

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
PROJECT_ROOT = Path(__file__).resolve().parent
BENCHMARK_PROGRAMS_DIR = PROJECT_ROOT / "benchmarks" / "wq"
BENCHMARK_PROGRAMS_REL_DIR = BENCHMARK_PROGRAMS_DIR.relative_to(PROJECT_ROOT).as_posix()
BENCHMARKS_ROOT = PROJECT_ROOT / ".benchmarks"
HISTORY_JSONL = BENCHMARKS_ROOT / "history.jsonl"
BINARY_NAME = "wq"
DEFAULT_PROFILE = "R"
DEFAULT_WARMUP = 1
DEFAULT_MIN_RUNS = 10

console = Console()

GIT_BLOB_HASH_CACHE: dict[tuple[str, str], str | None] = {}


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
def run(cmd: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
    console.print(f"[dim]$ {shlex.join(cmd)}[/dim]")
    return subprocess.run(cmd, capture_output=True, text=True, check=True, **kwargs)


def get_git_info() -> dict[str, str | bool | None]:
    def git(*args: str) -> str | None:
        try:
            return subprocess.check_output(
                ["git", *args],
                cwd=PROJECT_ROOT,
                text=True,
                stderr=subprocess.DEVNULL,
            ).strip()
        except subprocess.CalledProcessError:
            return None

    return {
        "commit": git("rev-parse", "--short", "HEAD"),
        "branch": git("rev-parse", "--abbrev-ref", "HEAD"),
        "tag": git("describe", "--tags", "--exact-match"),
        "dirty": git("diff", "--stat") not in (None, ""),
    }


def get_version(
    git_info: dict[str, str | bool | None],
    override: str | None = None,
) -> str:
    if override:
        return override
    tag = git_info.get("tag")
    if isinstance(tag, str) and tag:
        return tag
    commit = git_info.get("commit")
    return commit if isinstance(commit, str) and commit else "unknown"


def canonical_profile(profile: str) -> str:
    profile = profile.strip()
    if not profile:
        return DEFAULT_PROFILE
    if profile.lower() == "debug":
        return "dev"
    return profile


def profile_target_dir(profile: str) -> str:
    match profile.lower():
        case "dev":
            return "debug"
        case "release":
            return "release"
        case _:
            return profile


def binary_path_for_profile(profile: str) -> Path:
    return PROJECT_ROOT / "target" / profile_target_dir(profile) / BINARY_NAME


def resolve_binary(profile: str, binary_arg: Path | None) -> Path:
    if binary_arg is None:
        return binary_path_for_profile(profile)
    if binary_arg.is_absolute():
        return binary_arg
    return (PROJECT_ROOT / binary_arg).resolve()


def safe_dir_component(value: str) -> str:
    cleaned = "".join(c if c.isalnum() or c in "._-" else "_" for c in value)
    return cleaned or "unknown"


def project_display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(PROJECT_ROOT.resolve()).as_posix()
    except ValueError:
        return str(path)


def default_script_path(name: str) -> str:
    return f"{BENCHMARK_PROGRAMS_REL_DIR}/{name}.wq"


def script_path_from_values(
    name: str,
    vals: dict[str, Any],
    fallback: str | None = None,
) -> str:
    path = vals.get("script_path") or vals.get("path") or fallback
    return str(path or default_script_path(name))


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def benchmark_metadata(path: Path) -> dict[str, Any]:
    data = path.read_bytes()
    return {
        "script_path": path.relative_to(PROJECT_ROOT).as_posix(),
        "script_sha256": sha256_bytes(data),
        "script_size_bytes": len(data),
    }


def output_excerpt(stdout: str | None, stderr: str | None) -> str:
    parts = []
    for label, text in (("stdout", stdout), ("stderr", stderr)):
        if text:
            clean = " ".join(text.strip().splitlines())
            if len(clean) > 500:
                clean = f"{clean[:500]}..."
            if clean:
                parts.append(f"{label}: {clean}")
    return "\n".join(parts)


def benchmark_skip(
    name: str,
    metadata: dict[str, Any],
    reason: str,
    detail: str = "",
) -> dict[str, Any]:
    skipped = {
        "benchmark": name,
        "script_path": metadata.get("script_path"),
        "reason": reason,
    }
    if detail:
        skipped["detail"] = detail
    return skipped


def git_blob_sha256(commit: str, rel_path: str) -> str | None:
    key = (commit, rel_path)
    if key in GIT_BLOB_HASH_CACHE:
        return GIT_BLOB_HASH_CACHE[key]

    try:
        data = subprocess.check_output(
            ["git", "show", f"{commit}:{rel_path}"],
            cwd=PROJECT_ROOT,
            stderr=subprocess.DEVNULL,
        )
    except subprocess.CalledProcessError, FileNotFoundError:
        GIT_BLOB_HASH_CACHE[key] = None
        return None

    digest = sha256_bytes(data)
    GIT_BLOB_HASH_CACHE[key] = digest
    return digest


def script_hash_from_record(
    rec: dict[str, Any],
    name: str,
    vals: dict[str, Any],
    fallback_path: str | None = None,
) -> str | None:
    stored = vals.get("script_sha256") or vals.get("source_sha256")
    if isinstance(stored, str) and stored:
        return stored

    git_info = rec.get("git", {})
    if not isinstance(git_info, dict):
        return None
    if git_info.get("dirty") is True:
        return None

    commit = git_info.get("commit")
    if not isinstance(commit, str) or not commit:
        return None

    rel_path = script_path_from_values(name, vals, fallback_path)
    return git_blob_sha256(commit, rel_path)


def record_profile(rec: dict[str, Any]) -> str:
    profile = rec.get("profile")
    if profile is None and isinstance(rec.get("build"), dict):
        profile = rec["build"].get("profile")
    return canonical_profile(str(profile or DEFAULT_PROFILE))


def record_timestamp(rec: dict[str, Any]) -> datetime:
    raw = str(rec.get("timestamp") or "")
    try:
        parsed = datetime.fromisoformat(raw)
    except ValueError:
        return datetime.min.replace(tzinfo=UTC)
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=UTC)
    return parsed


def is_excluded(path: Path) -> bool:
    """Mirror of the Rust is_excluded logic."""
    exclude_comment = ["//exclude", "// exclude"]
    try:
        first = path.read_text(encoding="utf-8").splitlines()[0]
        trimmed = first.strip().lower()
        return any(trimmed.startswith(p) for p in exclude_comment)
    except IndexError, OSError, UnicodeError:
        return False


def collect_benchmarks() -> list[Path]:
    if not BENCHMARK_PROGRAMS_DIR.exists():
        console.print(f"[red]Directory not found: {BENCHMARK_PROGRAMS_DIR}[/red]")
        sys.exit(1)

    files = sorted(p for p in BENCHMARK_PROGRAMS_DIR.glob("*.wq") if not is_excluded(p))
    if not files:
        console.print("[yellow]No benchmark files found.[/yellow]")
        sys.exit(0)

    seen: dict[str, Path] = {}
    for path in files:
        if path.stem in seen:
            console.print(
                "[red]Duplicate benchmark name:[/red] "
                f"{path.stem} ({seen[path.stem]} and {path})"
            )
            sys.exit(1)
        seen[path.stem] = path

    return files


def build_with_profile(profile: str) -> None:
    console.print(Panel(f"[bold cyan]Building profile {profile}...[/bold cyan]"))
    run(["cargo", "build", "--profile", profile, "-p", "wq-cli"])
    console.print("[green]Build complete.[/green]\n")


def print_tree(path: Path, prefix: str = "") -> None:
    entries = sorted(path.iterdir(), key=lambda p: (p.is_file(), p.name.lower()))
    for idx, entry in enumerate(entries):
        is_last = idx == len(entries) - 1
        connector = "└── " if is_last else "├── "
        console.print(f"{prefix}{connector}{entry.name}")
        if entry.is_dir():
            print_tree(entry, prefix + ("    " if is_last else "│   "))


# ---------------------------------------------------------------------------
# Hyperfine runner (per-bench)
# ---------------------------------------------------------------------------
def run_hyperfine_individual(
    files: list[Path],
    benches_dir: Path,
    binary_path: Path,
    warmup: int = DEFAULT_WARMUP,
    min_runs: int = DEFAULT_MIN_RUNS,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """
    Run hyperfine for each benchmark individually so that every bench gets
    its own --export-json and --export-markdown files.
    Returns a tuple of successful result dicts and skipped benchmark dicts.
    """
    results: list[dict[str, Any]] = []
    skipped: list[dict[str, Any]] = []
    total = len(files)

    for idx, path in enumerate(files, 1):
        name = path.stem
        json_out = benches_dir / f"{name}.json"
        md_out = benches_dir / f"{name}.md"
        metadata = benchmark_metadata(path)

        console.print(
            f"[bold cyan][{idx}/{total}][/bold cyan] "
            f"Running hyperfine for [green]{name}[/green]..."
        )

        wq_cmd = [str(binary_path), str(path), "--no-bt"]
        try:
            preflight = subprocess.run(
                wq_cmd,
                capture_output=True,
                text=True,
                cwd=PROJECT_ROOT,
                check=False,
            )
        except OSError as exc:
            reason = f"could not run wq preflight: {exc}"
            skipped.append(benchmark_skip(name, metadata, reason))
            console.print(f"[yellow]Skipping {name}: {reason}[/yellow]")
            continue

        if preflight.returncode != 0:
            reason = f"wq exited with code {preflight.returncode}"
            detail = output_excerpt(preflight.stdout, preflight.stderr)
            skipped.append(benchmark_skip(name, metadata, reason, detail))
            console.print(f"[yellow]Skipping {name}: {reason}[/yellow]")
            continue

        bench_cmd = shlex.join(wq_cmd)
        cmd = [
            "hyperfine",
            f"--warmup={warmup}",
            f"--min-runs={min_runs}",
            "--time-unit=millisecond",
            f"--export-json={json_out}",
            f"--export-markdown={md_out}",
            f"--command-name={name}",
            bench_cmd,
        ]
        try:
            run(cmd)
        except subprocess.CalledProcessError as exc:
            reason = f"hyperfine failed with code {exc.returncode}"
            detail = output_excerpt(exc.stdout, exc.stderr)
            skipped.append(benchmark_skip(name, metadata, reason, detail))
            console.print(f"[yellow]Skipping {name}: {reason}[/yellow]")
            continue

        with open(json_out, encoding="utf-8") as fh:
            raw = json.load(fh)

        res = raw["results"][0]
        results.append(
            {
                "benchmark": name,
                "mean_ms": res["mean"] * 1000.0,
                "stddev_ms": res["stddev"] * 1000.0,
                "min_ms": res["min"] * 1000.0,
                "max_ms": res["max"] * 1000.0,
                "median_ms": res["median"] * 1000.0,
                "runs": len(res.get("times", [])),
                "json_file": str(json_out.relative_to(benches_dir.parent)),
                "md_file": str(md_out.relative_to(benches_dir.parent)),
                **metadata,
            }
        )

    return results, skipped


# ---------------------------------------------------------------------------
# Pretty print
# ---------------------------------------------------------------------------
def has_value(value: Any) -> bool:
    return value is not None and not (isinstance(value, float) and math.isnan(value))


def fmt_ms(value: Any) -> str:
    if not has_value(value):
        return "—"
    return f"{float(value):.2f}"


def fmt_delta(current: float, previous: Any) -> str:
    if not has_value(previous):
        return "—"
    previous_float = float(previous)
    if math.isclose(previous_float, 0.0):
        return "—"

    pct = ((current - previous_float) / previous_float) * 100.0
    style = "red" if pct > 2.0 else ("green" if pct < -2.0 else "dim")
    return f"[{style}]{pct:+.1f}%[/{style}]"


def status_cell(status: Any) -> str:
    match status:
        case "YES":
            return "[red]YES[/red]"
        case "IMPROVED":
            return "[green]IMPROVED[/green]"
        case "stable":
            return "[dim]stable[/dim]"
        case "single":
            return "[dim]baseline[/dim]"
        case "changed":
            return "[magenta]changed[/magenta]"
        case _:
            return "[blue]new[/blue]"


def print_skipped_benchmarks(skipped: list[dict[str, Any]]) -> None:
    if not skipped:
        return

    table = Table(
        title="Skipped Benchmarks",
        show_header=True,
        header_style="bold yellow",
    )
    table.add_column("Benchmark", style="cyan", no_wrap=True)
    table.add_column("Reason", style="yellow")
    table.add_column("Detail", style="dim")

    for row in skipped:
        table.add_row(
            str(row["benchmark"]),
            str(row["reason"]),
            str(row.get("detail") or "-"),
        )

    console.print(table)
    console.print(
        "[yellow]Skipped benchmarks are not included in history or regression "
        "analysis.[/yellow]"
    )


def print_results(
    rows: list[dict[str, Any]], reg_df: list[dict[str, Any]] | None = None
) -> int:
    analysis_lookup: dict[str, dict[str, Any]] = {}
    if reg_df:
        analysis_lookup = {str(r["benchmark"]): r for r in reg_df}

    table = Table(
        title="WQ Benchmark Results",
        show_header=True,
        header_style="bold magenta",
    )
    table.add_column("Benchmark", style="cyan", no_wrap=True)
    table.add_column("Current", justify="right", style="green")
    table.add_column("StdDev", justify="right", style="yellow")
    table.add_column("Previous", justify="right")
    table.add_column("Delta", justify="right")
    table.add_column("Hist Avg", justify="right")
    table.add_column("Hist N", justify="right", style="dim")
    table.add_column("Runs", justify="right", style="dim")
    table.add_column("Status", justify="center")
    table.add_column("Notes", style="dim")

    regressions = 0
    detached_total = 0
    unknown_total = 0

    for row in rows:
        name = str(row["benchmark"])
        mean = float(row["mean_ms"])
        stddev = float(row["stddev_ms"])
        info = analysis_lookup.get(name, {})

        status = info.get("regression")
        if status == "YES":
            regressions += 1

        detached = int(info.get("detached_count") or 0)
        unknown = int(info.get("unknown_count") or 0)
        detached_total += detached
        unknown_total += unknown

        notes = []
        if detached:
            notes.append(f"detached {detached}")
        if unknown:
            notes.append(f"unverified {unknown}")

        history_n = int(info.get("history_n") or 0)
        table.add_row(
            name,
            fmt_ms(mean),
            fmt_ms(stddev),
            fmt_ms(info.get("previous_ms")),
            fmt_delta(mean, info.get("previous_ms")),
            fmt_ms(info.get("history_mean_ms")),
            str(history_n),
            str(row.get("runs", "—")),
            status_cell(status),
            ", ".join(notes) if notes else "—",
        )

    console.print(table)
    if detached_total:
        console.print(
            "[yellow]Detached "
            f"{detached_total} past result(s) because benchmark contents changed."
            "[/yellow]"
        )
    if unknown_total:
        console.print(
            "[dim]Skipped "
            f"{unknown_total} legacy result(s) with unverifiable benchmark content."
            "[/dim]"
        )

    if regressions:
        console.print(f"\n[bold red]Detected {regressions} regression(s).[/bold red]")
    else:
        console.print("\n[bold green]No significant regressions detected.[/bold green]")
    return regressions


# ---------------------------------------------------------------------------
# Persistence
# ---------------------------------------------------------------------------
def make_history_record(
    rows: list[dict[str, Any]],
    git_info: dict[str, str | bool | None],
    version: str,
    timestamp: datetime,
    out_dir: Path,
    profile: str,
    binary_path: Path,
) -> dict[str, Any]:
    return {
        "timestamp": timestamp.isoformat(),
        "version": version,
        "profile": profile,
        "binary": project_display_path(binary_path),
        "run_dir": str(out_dir.relative_to(PROJECT_ROOT)),
        "git": git_info,
        "benchmarks": {
            row["benchmark"]: {
                "mean_ms": float(row["mean_ms"]),
                "stddev_ms": float(row["stddev_ms"]),
                "min_ms": float(row["min_ms"]),
                "max_ms": float(row["max_ms"]),
                "median_ms": float(row["median_ms"]),
                "runs": row.get("runs"),
                "script_path": row.get("script_path"),
                "script_sha256": row.get("script_sha256"),
                "script_size_bytes": row.get("script_size_bytes"),
            }
            for row in rows
        },
    }


def persist_results(
    rows: list[dict[str, Any]],
    record: dict[str, Any],
    out_dir: Path,
) -> None:
    BENCHMARKS_ROOT.mkdir(parents=True, exist_ok=True)

    csv_path = out_dir / "summary.csv"
    git_info = record.get("git", {})
    with open(csv_path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.writer(fh)
        writer.writerow(
            [
                "timestamp",
                "version",
                "profile",
                "binary",
                "git_commit",
                "git_branch",
                "benchmark",
                "script_path",
                "script_sha256",
                "script_size_bytes",
                "mean_ms",
                "stddev_ms",
                "min_ms",
                "max_ms",
                "median_ms",
                "runs",
            ]
        )
        for row in rows:
            writer.writerow(
                [
                    record["timestamp"],
                    record["version"],
                    record["profile"],
                    record["binary"],
                    git_info.get("commit") if isinstance(git_info, dict) else "",
                    git_info.get("branch") if isinstance(git_info, dict) else "",
                    row["benchmark"],
                    row.get("script_path", ""),
                    row.get("script_sha256", ""),
                    row.get("script_size_bytes", ""),
                    f"{row['mean_ms']:.6f}",
                    f"{row['stddev_ms']:.6f}",
                    f"{row['min_ms']:.6f}",
                    f"{row['max_ms']:.6f}",
                    f"{row['median_ms']:.6f}",
                    row["runs"],
                ]
            )
    console.print(f"[dim]Summary CSV: {csv_path}[/dim]")

    with open(HISTORY_JSONL, "a", encoding="utf-8") as fh:
        fh.write(json.dumps(record) + "\n")
    console.print(f"[dim]History JSONL: {HISTORY_JSONL}[/dim]")


def write_skipped_report(skipped: list[dict[str, Any]], out_dir: Path) -> None:
    if not skipped:
        return

    skipped_path = out_dir / "skipped.json"
    skipped_path.write_text(json.dumps(skipped, indent=2), encoding="utf-8")
    console.print(f"[dim]Skipped report: {skipped_path}[/dim]")


# ---------------------------------------------------------------------------
# Visualization
# ---------------------------------------------------------------------------
def should_use_log_scale(values: list[float]) -> bool:
    positive = [value for value in values if value > 0]
    if len(positive) < 2:
        return False
    return max(positive) / min(positive) > 50.0


def generate_visualizations(
    rows: list[dict[str, Any]],
    history: list[dict[str, Any]],
    plots_dir: Path,
    profile: str,
) -> None:
    if not rows:
        return

    try:
        import matplotlib.pyplot as plt
    except ModuleNotFoundError:
        console.print(
            "[yellow]matplotlib is not installed; skipping plots. "
            "Install it with `python3 -m pip install matplotlib`.[/yellow]"
        )
        return

    # --- 1. Bar chart: current run -----------------------------------------
    summary_rows = sorted(rows, key=lambda row: float(row["mean_ms"]), reverse=True)
    names = [str(row["benchmark"]) for row in summary_rows]
    means = [float(row["mean_ms"]) for row in summary_rows]
    stds = [float(row["stddev_ms"]) for row in summary_rows]
    height = max(6.0, min(24.0, 2.8 + len(summary_rows) * 0.28))
    fig, ax = plt.subplots(figsize=(12, height), constrained_layout=True)
    ax.barh(
        names,
        means,
        xerr=stds,
        capsize=2,
        color="#4C78A8",
        edgecolor="#2F3A45",
        linewidth=0.6,
    )
    ax.invert_yaxis()
    ax.grid(axis="x", alpha=0.25)
    if should_use_log_scale(means):
        ax.set_xscale("log")
        ax.set_xlabel("Time (ms, log scale)")
    else:
        ax.set_xlabel("Time (ms)")
    ax.set_ylabel("")
    ax.set_title(f"WQ Benchmarks - Current Run ({profile})")

    bar_path = plots_dir / "summary.png"
    fig.savefig(bar_path, dpi=160)
    plt.close(fig)
    console.print(f"[dim]Bar chart: {bar_path}[/dim]")

    # --- 2. Regression trend lines (if enough matching history) -------------
    series = trend_series_for_current_rows(rows, history, profile)
    if not series:
        return

    legend_cols = min(5, max(1, math.ceil(len(series) / 14)))
    legend_rows = math.ceil(len(series) / legend_cols)
    fig_height = max(6.0, min(16.0, 5.0 + legend_rows * 0.18))
    fig, ax = plt.subplots(figsize=(13, fig_height))
    cmap = plt.get_cmap("tab20")
    all_y: list[float] = []

    for idx, (bench, points) in enumerate(series):
        timestamps = [point["timestamp"] for point in points]
        mean_values = [float(point["mean_ms"]) for point in points]
        all_y.extend(mean_values)
        ax.plot(
            timestamps,
            mean_values,
            marker="o",
            markersize=3,
            linewidth=1.2,
            label=bench,
            color=cmap(idx % 20),
            alpha=0.85,
        )

    if should_use_log_scale(all_y):
        ax.set_yscale("log")
        ax.set_ylabel("Mean time (ms, log scale)")
    else:
        ax.set_ylabel("Mean time (ms)")
    ax.set_xlabel("Time")
    ax.set_title(f"WQ Benchmarks - Historical Trend ({profile})")
    ax.grid(axis="y", alpha=0.25)
    ax.margins(x=0.02)
    ax.legend(
        loc="upper center",
        bbox_to_anchor=(0.5, -0.18),
        ncol=legend_cols,
        fontsize="x-small",
        frameon=False,
        handlelength=1.4,
        columnspacing=1.0,
    )

    bottom = min(0.48, 0.12 + legend_rows * 0.025)
    fig.autofmt_xdate()
    fig.subplots_adjust(left=0.08, right=0.98, top=0.9, bottom=bottom)

    trend_path = plots_dir / "trend.png"
    fig.savefig(trend_path, dpi=160)
    plt.close(fig)
    console.print(f"[dim]Trend chart: {trend_path}[/dim]")


# ---------------------------------------------------------------------------
# Regression analysis
# ---------------------------------------------------------------------------
def load_history() -> list[dict[str, Any]]:
    if not HISTORY_JSONL.exists():
        return []
    records = []
    with open(HISTORY_JSONL, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                records.append(json.loads(line))
    return records


def matching_history_samples(
    row: dict[str, Any],
    history: list[dict[str, Any]],
    profile: str,
) -> tuple[list[tuple[dict[str, Any], float]], int, int]:
    bench = str(row["benchmark"])
    current_hash = row.get("script_sha256")
    current_path = str(row.get("script_path") or default_script_path(bench))

    samples: list[tuple[dict[str, Any], float]] = []
    detached = 0
    unknown = 0

    for rec in sorted(history, key=record_timestamp):
        if record_profile(rec) != profile:
            continue

        bench_vals = rec.get("benchmarks", {}).get(bench)
        if not isinstance(bench_vals, dict) or "mean_ms" not in bench_vals:
            continue

        if current_hash:
            past_hash = script_hash_from_record(
                rec,
                bench,
                bench_vals,
                current_path,
            )
            if past_hash:
                if past_hash != current_hash:
                    detached += 1
                    continue
            else:
                unknown += 1
                continue

        samples.append((rec, float(bench_vals["mean_ms"])))

    return samples, detached, unknown


def analyze_regression(
    rows: list[dict[str, Any]],
    history: list[dict[str, Any]],
    profile: str,
) -> list[dict[str, Any]]:
    profile_runs = sum(1 for rec in history if record_profile(rec) == profile)
    if profile_runs == 0:
        console.print(f"[dim]No previous {profile} history found.[/dim]")
    elif profile_runs < 2:
        console.print(
            f"[dim]Only {profile_runs} previous {profile} run(s); "
            "z-score needs >=2 matching runs.[/dim]"
        )

    stats: list[dict[str, Any]] = []
    for row in rows:
        bench = row["benchmark"]
        samples, detached, unknown = matching_history_samples(row, history, profile)
        vals = [value for _, value in samples]
        current = float(row["mean_ms"])

        hist_mean: float | None = None
        hist_std: float | None = None
        z_score: float | None = None
        previous_ms: float | None = vals[-1] if vals else None
        previous_version = samples[-1][0].get("version") if samples else None

        if len(vals) >= 2:
            hist_mean = statistics.mean(vals)
            hist_std = statistics.stdev(vals)
            if hist_std and not math.isclose(hist_std, 0.0):
                z_score = (current - hist_mean) / hist_std
            else:
                z_score = 0.0

            regression = (
                "YES" if z_score > 2.0 else ("IMPROVED" if z_score < -2.0 else "stable")
            )
        elif len(vals) == 1:
            hist_mean = vals[0]
            hist_std = 0.0
            regression = "single"
        elif detached:
            regression = "changed"
        else:
            regression = "new"

        stats.append(
            {
                "benchmark": bench,
                "history_mean_ms": hist_mean,
                "history_std_ms": hist_std,
                "current_ms": current,
                "previous_ms": previous_ms,
                "previous_version": previous_version,
                "history_n": len(vals),
                "z_score": z_score,
                "regression": regression,
                "detached_count": detached,
                "unknown_count": unknown,
            }
        )

    return stats


def trend_series_for_current_rows(
    rows: list[dict[str, Any]],
    history: list[dict[str, Any]],
    profile: str,
) -> list[tuple[str, list[dict[str, Any]]]]:
    series: list[tuple[str, list[dict[str, Any]]]] = []

    for row in rows:
        bench = str(row["benchmark"])
        current_hash = row.get("script_sha256")
        current_path = str(row.get("script_path") or default_script_path(bench))
        points: list[dict[str, Any]] = []

        for rec in sorted(history, key=record_timestamp):
            if record_profile(rec) != profile:
                continue

            vals = rec.get("benchmarks", {}).get(bench)
            if not isinstance(vals, dict) or "mean_ms" not in vals:
                continue

            if current_hash:
                past_hash = script_hash_from_record(rec, bench, vals, current_path)
                if not past_hash or past_hash != current_hash:
                    continue

            points.append(
                {
                    "timestamp": record_timestamp(rec),
                    "mean_ms": float(vals["mean_ms"]),
                }
            )

        if len(points) >= 2:
            points = sorted(points, key=lambda point: point["timestamp"])
            series.append((bench, points))

    return series


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="WQ benchmark runner powered by hyperfine")
    p.add_argument(
        "--profile",
        type=str,
        default=None,
        help=f"Cargo profile to build and run (default: {DEFAULT_PROFILE}; debug=dev)",
    )
    p.add_argument(
        "--binary",
        type=Path,
        default=None,
        help="Use this wq binary instead of target/<profile>/wq",
    )
    p.add_argument(
        "--no-build",
        action="store_true",
        help="Skip cargo build for the selected profile",
    )
    p.add_argument(
        "--warmup", type=int, default=DEFAULT_WARMUP, help="Hyperfine warmup runs"
    )
    p.add_argument(
        "--min-runs", type=int, default=DEFAULT_MIN_RUNS, help="Hyperfine minimum runs"
    )
    p.add_argument("--version", type=str, default=None, help="Override version string")
    p.add_argument("--no-plots", action="store_true", help="Skip generating plots")
    p.add_argument(
        "--no-persist", action="store_true", help="Skip saving to history files"
    )
    p.add_argument(
        "--analyze-only",
        action="store_true",
        help="Do not run benchmarks; only analyze history and regenerate plots",
    )

    args = p.parse_args()
    if args.warmup < 0:
        p.error("--warmup must be >= 0")
    if args.min_runs < 1:
        p.error("--min-runs must be >= 1")
    if args.profile is not None and not args.profile.strip():
        p.error("--profile must not be empty")
    return args


def _resolve_run_dir_from_record(rec: dict[str, Any]) -> Path:
    """Find the run directory for a history record."""
    if "run_dir" in rec:
        return PROJECT_ROOT / rec["run_dir"]

    version = rec.get("version", "unknown")
    profile = record_profile(rec)
    ts = rec.get("timestamp", "")
    try:
        ts_str = datetime.fromisoformat(str(ts)).strftime("%Y%m%d_%H%M%S")
    except ValueError:
        ts_str = "unknown"
    return BENCHMARKS_ROOT / f"v{version}_{profile}_{ts_str}"


def _rows_from_record(rec: dict[str, Any]) -> list[dict[str, Any]]:
    """Convert a history record back to the row format used by print_results."""
    rows = []
    for name, vals in rec.get("benchmarks", {}).items():
        if not isinstance(vals, dict):
            continue
        script_path = script_path_from_values(name, vals)
        rows.append(
            {
                "benchmark": name,
                "mean_ms": vals["mean_ms"],
                "stddev_ms": vals["stddev_ms"],
                "min_ms": vals["min_ms"],
                "max_ms": vals["max_ms"],
                "median_ms": vals["median_ms"],
                "runs": vals.get("runs", "—"),
                "script_path": script_path,
                "script_sha256": script_hash_from_record(
                    rec,
                    name,
                    vals,
                    script_path,
                ),
                "script_size_bytes": vals.get("script_size_bytes"),
            }
        )
    return rows


def run_analyze_only(args: argparse.Namespace) -> int:
    history = load_history()
    if not history:
        console.print("[red]No history found. Run without --analyze-only first.[/red]")
        return 1

    requested_profile = canonical_profile(args.profile) if args.profile else None
    candidates = [
        (idx, rec)
        for idx, rec in enumerate(history)
        if requested_profile is None or record_profile(rec) == requested_profile
    ]
    if not candidates:
        console.print(f"[red]No history found for profile {requested_profile}.[/red]")
        return 1

    latest_idx, latest = candidates[-1]
    profile = record_profile(latest)
    run_dir = _resolve_run_dir_from_record(latest)
    if not run_dir.exists():
        console.print(
            f"[yellow]Run directory not found ({run_dir}); creating fallback.[/yellow]"
        )
        run_dir.mkdir(parents=True, exist_ok=True)

    plots_dir = run_dir / "plots"
    plots_dir.mkdir(parents=True, exist_ok=True)

    version = latest.get("version", "unknown")
    ts = latest.get("timestamp", "")
    console.print(
        Panel(
            f"[bold cyan]ANALYZE ONLY[/bold cyan]\n"
            f"[bold cyan]Version:[/bold cyan] {version}\n"
            f"[bold cyan]Profile:[/bold cyan] {profile}\n"
            f"[bold cyan]Time:[/bold cyan]    {ts}\n"
            f"[bold cyan]Output:[/bold cyan]  {run_dir}"
        )
    )

    rows = _rows_from_record(latest)
    reg_df = analyze_regression(rows, history[:latest_idx], profile)
    regressions = print_results(rows, reg_df)

    if not args.no_plots:
        generate_visualizations(rows, history[: latest_idx + 1], plots_dir, profile)

    console.print(f"\n[bold]Artifacts in {run_dir}:[/bold]")
    print_tree(run_dir)
    return 1 if regressions > 0 else 0


def main() -> int:
    args = parse_args()

    if args.analyze_only:
        return run_analyze_only(args)

    profile = canonical_profile(args.profile or DEFAULT_PROFILE)
    binary_path = resolve_binary(profile, args.binary)

    if args.binary is not None and not args.no_build:
        console.print("[dim]--binary supplied; skipping cargo build.[/dim]")
    elif not args.no_build:
        build_with_profile(profile)

    if not binary_path.exists():
        console.print(f"[red]Binary not found: {binary_path}[/red]")
        return 1

    files = collect_benchmarks()
    git_info = get_git_info()
    version = get_version(git_info, args.version)
    ts = datetime.now(UTC)
    ts_str = ts.strftime("%Y%m%d_%H%M%S")
    run_dir = BENCHMARKS_ROOT / (
        f"v{safe_dir_component(version)}_{safe_dir_component(profile)}_{ts_str}"
    )
    run_dir.mkdir(parents=True, exist_ok=True)

    console.print(
        Panel(
            f"[bold cyan]Version:[/bold cyan] {version}\n"
            f"[bold cyan]Profile:[/bold cyan] {profile}\n"
            f"[bold cyan]Binary:[/bold cyan]  {project_display_path(binary_path)}\n"
            f"[bold cyan]Commit:[/bold cyan]  "
            f"{git_info.get('commit')} ({git_info.get('branch')})\n"
            f"[bold cyan]Time:[/bold cyan]    {ts.isoformat()}\n"
            f"[bold cyan]Output:[/bold cyan]  {run_dir}"
        )
    )

    benches_dir = run_dir / "benches"
    benches_dir.mkdir(parents=True, exist_ok=True)
    plots_dir = run_dir / "plots"
    plots_dir.mkdir(parents=True, exist_ok=True)

    history_before = load_history()
    rows, skipped = run_hyperfine_individual(
        files,
        benches_dir=benches_dir,
        binary_path=binary_path,
        warmup=args.warmup,
        min_runs=args.min_runs,
    )
    write_skipped_report(skipped, run_dir)
    print_skipped_benchmarks(skipped)

    if not rows:
        console.print("[red]No benchmarks completed successfully.[/red]")
        console.print(f"\n[bold]Artifacts in {run_dir}:[/bold]")
        print_tree(run_dir)
        return 1

    reg_df = analyze_regression(rows, history_before, profile)
    regressions = print_results(rows, reg_df)

    current_record = make_history_record(
        rows,
        git_info,
        version,
        ts,
        run_dir,
        profile,
        binary_path,
    )
    if not args.no_persist:
        persist_results(rows, current_record, run_dir)

    if not args.no_plots:
        generate_visualizations(
            rows,
            [*history_before, current_record],
            plots_dir,
            profile,
        )

    console.print(f"\n[bold]Artifacts in {run_dir}:[/bold]")
    print_tree(run_dir)

    return 1 if regressions > 0 else 0


if __name__ == "__main__":
    sys.exit(main())
