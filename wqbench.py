#!/usr/bin/env python3
"""
WQ Benchmark Runner

A hyperfine-based benchmarking script for the wq interpreter.

------------------------------------------------------------------------------
DEPENDENCIES
------------------------------------------------------------------------------
    pip install rich matplotlib pandas

    Also requires hyperfine and cargo to be installed:
        brew install hyperfine   # macOS
        cargo build --release    # builds target/release/wq

------------------------------------------------------------------------------
USAGE
------------------------------------------------------------------------------
    # Full benchmark run (builds release binary, runs all benches, generates plots)
    python3 wq_bench.py

    # Skip cargo build (use existing target/release/wq)
    python3 wq_bench.py --no-build

    # Quick validation with fewer runs
    python3 wq_bench.py --no-build --min-runs 3 --warmup 1

    # Override version string (default: git tag > commit hash)
    python3 wq_bench.py --version v1.2.0

    # Update the old tests/benchmarks.txt baseline after review
    python3 wq_bench.py --update-baseline

    # Skip plots or persistence (useful in CI when you only want the table)
    python3 wq_bench.py --no-plots --no-persist

    # Re-analyze existing history and (re)generate trend.png
    python3 wq_bench.py --analyze-only

------------------------------------------------------------------------------
DIRECTORY LAYOUT
------------------------------------------------------------------------------
    .benchmarks/
    ├── v1.0.0_20260101_120000/     ← one directory per run
    │   ├── benches/                  ← per-benchmark hyperfine exports
    │   │   ├── 1.json                ← hyperfine --export-json
    │   │   ├── 1.md                  ← hyperfine --export-markdown (paste into PRs)
    │   │   ├── bf.json
    │   │   ├── bf.md
    │   │   └── ...
    │   ├── plots/                    ← visualization outputs
    │   │   ├── summary.png           ← bar chart of current run
    │   │   └── trend.png             ← historical trend (≥2 runs)
    │   └── summary.csv               ← aggregated results for this run
    ├── v4b99994_20260425_152835/
    │   └── ...
    └── history.jsonl                 ← slim history for regression analysis

    # Re-analyze latest run without re-executing benchmarks
    python3 wq_bench.py --analyze-only

------------------------------------------------------------------------------
OUTPUTS
------------------------------------------------------------------------------
    • benches/*.json      Full hyperfine statistics (mean, stddev, all timings)
    • benches/*.md        Markdown tables ready to paste into GitHub PRs
    • summary.csv         CSV with all benchmarks for this run
    • summary.png         Bar chart with error bars
    • trend.png           Historical trend lines (only when ≥2 runs exist)
    • history.jsonl       Append-only log of all runs for z-score regression

------------------------------------------------------------------------------
REGRESSION ANALYSIS
------------------------------------------------------------------------------
    Two layers of regression detection:

    1. Traditional threshold (vs tests/benchmarks.txt):
       Flags if >20% slower AND absolute difference >5ms.
       Matches the old Rust test behavior.

    2. Statistical z-score (vs history.jsonl):
       Computes historical mean/stddev across all past runs.
       z-score > 2  → "YES" (regression)
       z-score < -2 → "IMPROVED"
       Otherwise    → "stable"

    Exit code is 1 if any traditional regression is detected.

------------------------------------------------------------------------------
VERSION NAMING
------------------------------------------------------------------------------
    The directory name is "v{version}_{YYYYMMDD}_{HHMMSS}".
    Version resolution order:
        1. --version CLI flag
        2. git tag that exactly matches HEAD
        3. short git commit hash
"""

from __future__ import annotations

import argparse
import csv
import json

# import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import matplotlib.pyplot as plt
import pandas as pd
from rich.console import Console
from rich.panel import Panel
from rich.table import Table

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
PROJECT_ROOT = Path(__file__).resolve().parent
WQ_TESTS_DIR = PROJECT_ROOT / "hotchoco" / "wq"
BINARY_PATH = PROJECT_ROOT / "target" / "R" / "wq"
BENCHMARKS_ROOT = PROJECT_ROOT / ".benchmarks"
HISTORY_JSONL = BENCHMARKS_ROOT / "history.jsonl"
DEFAULT_WARMUP = 1
DEFAULT_MIN_RUNS = 10

console = Console()


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
def run(cmd: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
    console.print(f"[dim]$ {' '.join(cmd)}[/dim]")
    return subprocess.run(cmd, capture_output=True, text=True, check=True, **kwargs)


def get_git_info() -> dict[str, str | None]:
    def git(*args: str) -> str | None:
        try:
            return subprocess.check_output(
                ["git", *args], cwd=PROJECT_ROOT, text=True, stderr=subprocess.DEVNULL
            ).strip()
        except subprocess.CalledProcessError:
            return None

    return {
        "commit": git("rev-parse", "--short", "HEAD"),
        "branch": git("rev-parse", "--abbrev-ref", "HEAD"),
        "tag": git("describe", "--tags", "--exact-match"),
        "dirty": git("diff", "--stat") not in (None, ""),
    }


def get_version(git_info: dict[str, str | None], override: str | None = None) -> str:
    if override:
        return override
    if git_info.get("tag"):
        return git_info["tag"]
    return git_info.get("commit") or "unknown"


def is_excluded(path: Path) -> bool:
    """Mirror of the Rust is_excluded logic."""
    exclude_comment = ["//exclude", "// exclude"]
    try:
        first = path.read_text(encoding="utf-8").splitlines()[0]
        trimmed = first.strip().lower()
        return any(trimmed.startswith(p) for p in exclude_comment)
    except Exception:
        return False


def collect_benchmarks() -> list[Path]:
    if not WQ_TESTS_DIR.exists():
        console.print(f"[red]Directory not found: {WQ_TESTS_DIR}[/red]")
        sys.exit(1)
    files = sorted(p for p in WQ_TESTS_DIR.glob("*.wq") if not is_excluded(p))
    if not files:
        console.print("[yellow]No benchmark files found.[/yellow]")
        sys.exit(0)
    return files


def build_with_profile_R() -> None:
    console.print(Panel("[bold cyan]Building release binary…[/bold cyan]"))
    run(["cargo", "build", "--profile", "R", "-p", "wq-cli"])
    console.print("[green]Build complete.[/green]\n")


# ---------------------------------------------------------------------------
# Hyperfine runner (per-bench)
# ---------------------------------------------------------------------------
def run_hyperfine_individual(
    files: list[Path],
    benches_dir: Path,
    warmup: int = DEFAULT_WARMUP,
    min_runs: int = DEFAULT_MIN_RUNS,
) -> list[dict[str, Any]]:
    """
    Run hyperfine for each benchmark individually so that every bench gets
    its own --export-json and --export-markdown files.
    Returns a list of result dicts.
    """
    results: list[dict[str, Any]] = []
    total = len(files)

    for idx, path in enumerate(files, 1):
        name = path.stem
        json_out = benches_dir / f"{name}.json"
        md_out = benches_dir / f"{name}.md"

        console.print(
            f"[bold cyan][{idx}/{total}][/bold cyan] Running hyperfine for [green]{name}[/green]…"
        )

        cmd = [
            "hyperfine",
            f"--warmup={warmup}",
            f"--min-runs={min_runs}",
            "--time-unit=millisecond",
            f"--export-json={json_out}",
            f"--export-markdown={md_out}",
            f"--command-name={name}",
            f"{BINARY_PATH} {path} --no-bt",
        ]
        run(cmd)

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
            }
        )

    return results


# ---------------------------------------------------------------------------
# Pretty print
# ---------------------------------------------------------------------------
def print_results(
    rows: list[dict[str, Any]], reg_df: pd.DataFrame | None = None
) -> int:
    reg_lookup: dict[str, str] = {}
    if reg_df is not None and not reg_df.empty:
        for _, r in reg_df.iterrows():
            reg_lookup[r["benchmark"]] = r["regression"]

    table = Table(
        title="WQ Benchmark Results", show_header=True, header_style="bold magenta"
    )
    table.add_column("Benchmark", style="cyan", no_wrap=True)
    table.add_column("Mean (ms)", justify="right", style="green")
    table.add_column("StdDev (ms)", justify="right", style="yellow")
    table.add_column("Min (ms)", justify="right", style="dim")
    table.add_column("Max (ms)", justify="right", style="dim")
    table.add_column("Runs", justify="right", style="dim")
    table.add_column("Regression", justify="center")

    regressions = 0
    for row in rows:
        name = row["benchmark"]
        mean = float(row["mean_ms"])
        stddev = float(row["stddev_ms"])
        min_ms = float(row["min_ms"])
        max_ms = float(row["max_ms"])
        runs = row["runs"]

        reg_status = reg_lookup.get(name)
        if reg_status == "YES":
            reg_cell = "[red]YES[/red]"
            regressions += 1
        elif reg_status == "IMPROVED":
            reg_cell = "[green]IMPROVED[/green]"
        elif reg_status == "stable":
            reg_cell = "[dim]stable[/dim]"
        else:
            reg_cell = "[blue]new[/blue]"

        table.add_row(
            name,
            f"{mean:.2f}",
            f"{stddev:.2f}",
            f"{min_ms:.2f}",
            f"{max_ms:.2f}",
            str(runs),
            reg_cell,
        )

    console.print(table)
    if regressions:
        console.print(f"\n[bold red]Detected {regressions} regression(s).[/bold red]")
    else:
        console.print("\n[bold green]No significant regressions detected.[/bold green]")
    return regressions


# ---------------------------------------------------------------------------
# Persistence
# ---------------------------------------------------------------------------
def persist_results(
    rows: list[dict[str, Any]],
    git_info: dict[str, str | None],
    version: str,
    timestamp: datetime,
    out_dir: Path,
    args: argparse.Namespace,
) -> None:
    BENCHMARKS_ROOT.mkdir(parents=True, exist_ok=True)

    # 1. CSV summary inside the versioned directory
    csv_path = out_dir / "summary.csv"
    with open(csv_path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.writer(fh)
        writer.writerow(
            [
                "timestamp",
                "version",
                "git_commit",
                "git_branch",
                "benchmark",
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
                    timestamp.isoformat(),
                    version,
                    git_info.get("commit") or "unknown",
                    git_info.get("branch") or "unknown",
                    row["benchmark"],
                    f"{row['mean_ms']:.6f}",
                    f"{row['stddev_ms']:.6f}",
                    f"{row['min_ms']:.6f}",
                    f"{row['max_ms']:.6f}",
                    f"{row['median_ms']:.6f}",
                    row["runs"],
                ]
            )
    console.print(f"[dim]Summary CSV: {csv_path}[/dim]")

    # 2. JSONL history (slim, for regression analysis)
    record = {
        "timestamp": timestamp.isoformat(),
        "version": version,
        "run_dir": str(out_dir.relative_to(PROJECT_ROOT)),
        "git": git_info,
        "benchmarks": {
            row["benchmark"]: {
                "mean_ms": float(row["mean_ms"]),
                "stddev_ms": float(row["stddev_ms"]),
                "min_ms": float(row["min_ms"]),
                "max_ms": float(row["max_ms"]),
                "median_ms": float(row["median_ms"]),
            }
            for row in rows
        },
    }
    with open(HISTORY_JSONL, "a", encoding="utf-8") as fh:
        fh.write(json.dumps(record) + "\n")
    console.print(f"[dim]History JSONL: {HISTORY_JSONL}[/dim]")


# ---------------------------------------------------------------------------
# Visualization
# ---------------------------------------------------------------------------
def generate_visualizations(
    rows: list[dict[str, Any]],
    history: list[dict[str, Any]],
    plots_dir: Path,
) -> None:
    df = pd.DataFrame(rows)

    # --- 1. Bar chart: current run -----------------------------------------
    fig, ax = plt.subplots(figsize=(max(10, len(df) * 0.4), 6))
    names = df["benchmark"].tolist()
    means = df["mean_ms"].astype(float).tolist()
    stds = df["stddev_ms"].astype(float).tolist()
    ax.bar(names, means, yerr=stds, capsize=3, color="steelblue", edgecolor="black")
    ax.set_ylabel("Time (ms)")
    ax.set_title("WQ Benchmarks — Current Run")
    ax.tick_params(axis="x", rotation=45)
    fig.tight_layout()
    bar_path = plots_dir / "summary.png"
    fig.savefig(bar_path, dpi=150)
    plt.close(fig)
    console.print(f"[dim]Bar chart: {bar_path}[/dim]")

    # --- 2. Regression trend lines (if enough history) ----------------------
    if len(history) >= 2:
        fig, ax = plt.subplots(figsize=(12, 6))
        rows_hist: list[dict[str, Any]] = []
        for rec in history:
            ts = rec["timestamp"]
            for b, vals in rec["benchmarks"].items():
                rows_hist.append(
                    {
                        "timestamp": pd.to_datetime(ts),
                        "benchmark": b,
                        "mean_ms": vals["mean_ms"],
                    }
                )
        hist_df = pd.DataFrame(rows_hist)

        for bench in df["benchmark"]:
            sub = hist_df[hist_df["benchmark"] == bench].sort_values("timestamp")
            if len(sub) >= 2:
                ax.plot(
                    sub["timestamp"],
                    sub["mean_ms"],
                    marker="o",
                    label=bench,
                    alpha=0.7,
                )

        ax.set_xlabel("Time")
        ax.set_ylabel("Mean time (ms)")
        ax.set_title("WQ Benchmarks — Historical Trend")
        ax.legend(bbox_to_anchor=(1.05, 1), loc="upper left", fontsize="small")
        fig.tight_layout()
        trend_path = plots_dir / "trend.png"
        fig.savefig(trend_path, dpi=150)
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


def analyze_regression(
    rows: list[dict[str, Any]], history: list[dict[str, Any]]
) -> pd.DataFrame:
    if len(history) < 2:
        console.print(
            "[dim]Not enough history for statistical regression (need ≥2 runs).[/dim]"
        )
        return pd.DataFrame()

    stats: list[dict[str, Any]] = []
    for row in rows:
        bench = row["benchmark"]
        vals = [
            rec["benchmarks"][bench]["mean_ms"]
            for rec in history
            if bench in rec.get("benchmarks", {})
        ]
        if len(vals) < 2:
            continue
        series = pd.Series(vals)
        hist_mean = series.mean()
        hist_std = series.std()
        current = float(row["mean_ms"])
        z_score = (current - hist_mean) / hist_std if hist_std else 0.0
        stats.append(
            {
                "benchmark": bench,
                "history_mean_ms": round(hist_mean, 2),
                "history_std_ms": round(hist_std, 2),
                "current_ms": round(current, 2),
                "z_score": round(z_score, 2),
                "regression": (
                    "YES"
                    if z_score > 2.0
                    else ("IMPROVED" if z_score < -2.0 else "stable")
                ),
            }
        )

    return pd.DataFrame(stats)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="WQ benchmark runner powered by hyperfine")
    p.add_argument("--no-build", action="store_true", help="Skip cargo build --release")
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
        "--update-baseline", action="store_true", help="Update tests/benchmarks.txt"
    )
    p.add_argument(
        "--analyze-only",
        action="store_true",
        help="Do not run benchmarks; only analyze existing history.jsonl and (re)generate plots",
    )
    return p.parse_args()


def _resolve_run_dir_from_record(rec: dict[str, Any]) -> Path:
    """Find the run directory for a history record."""
    if "run_dir" in rec:
        return PROJECT_ROOT / rec["run_dir"]
    # fallback: derive from version + timestamp
    version = rec.get("version", "unknown")
    ts = rec.get("timestamp", "")
    try:
        ts_str = datetime.fromisoformat(ts).strftime("%Y%m%d_%H%M%S")
    except ValueError:
        ts_str = "unknown"
    return BENCHMARKS_ROOT / f"v{version}_{ts_str}"


def _rows_from_record(rec: dict[str, Any]) -> list[dict[str, Any]]:
    """Convert a history record back to the row format used by print_results."""
    rows = []
    for name, vals in rec.get("benchmarks", {}).items():
        rows.append(
            {
                "benchmark": name,
                "mean_ms": vals["mean_ms"],
                "stddev_ms": vals["stddev_ms"],
                "min_ms": vals["min_ms"],
                "max_ms": vals["max_ms"],
                "median_ms": vals["median_ms"],
                "runs": "—",
            }
        )
    return rows


def main() -> int:
    args = parse_args()

    # ------------------------------------------------------------------
    # Analyze-only mode: no builds, no hyperfine, just plots + stats
    # ------------------------------------------------------------------
    if args.analyze_only:
        history = load_history()
        if not history:
            console.print(
                "[red]No history found. Run without --analyze-only first.[/red]"
            )
            return 1

        latest = history[-1]
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
                f"[bold cyan]Time:[/bold cyan]    {ts}\n"
                f"[bold cyan]Output:[/bold cyan]  {run_dir}"
            )
        )

        rows = _rows_from_record(latest)
        reg_df = analyze_regression(rows, history)
        regressions = print_results(rows, reg_df)

        if not args.no_plots:
            generate_visualizations(rows, history, plots_dir)

        console.print(f"\n[bold]Artifacts in {run_dir}:[/bold]")

        def tree(path: Path, prefix: str = "") -> None:
            entries = sorted(
                path.iterdir(), key=lambda p: (p.is_file(), p.name.lower())
            )
            for idx, entry in enumerate(entries):
                is_last = idx == len(entries) - 1
                connector = "└── " if is_last else "├── "
                console.print(f"{prefix}{connector}{entry.name}")
                if entry.is_dir():
                    tree(entry, prefix + ("    " if is_last else "│   "))

        tree(run_dir)
        return 1 if regressions > 0 else 0

    # ------------------------------------------------------------------
    # Normal benchmark mode
    # ------------------------------------------------------------------
    if not args.no_build:
        build_with_profile_R()

    if not BINARY_PATH.exists():
        console.print(f"[red]Binary not found: {BINARY_PATH}[/red]")
        return 1

    files = collect_benchmarks()

    git_info = get_git_info()
    version = get_version(git_info, args.version)
    ts = datetime.now(timezone.utc)
    ts_str = ts.strftime("%Y%m%d_%H%M%S")
    run_dir = BENCHMARKS_ROOT / f"v{version}_{ts_str}"
    run_dir.mkdir(parents=True, exist_ok=True)

    console.print(
        Panel(
            f"[bold cyan]Version:[/bold cyan] {version}\n"
            f"[bold cyan]Commit:[/bold cyan]  {git_info.get('commit')} ({git_info.get('branch')})\n"
            f"[bold cyan]Time:[/bold cyan]    {ts.isoformat()}\n"
            f"[bold cyan]Output:[/bold cyan]  {run_dir}"
        )
    )

    benches_dir = run_dir / "benches"
    benches_dir.mkdir(parents=True, exist_ok=True)
    plots_dir = run_dir / "plots"
    plots_dir.mkdir(parents=True, exist_ok=True)

    rows = run_hyperfine_individual(
        files,
        benches_dir=benches_dir,
        warmup=args.warmup,
        min_runs=args.min_runs,
    )

    if not args.no_persist:
        persist_results(rows, git_info, version, ts, run_dir, args)

    history = load_history()
    reg_df = analyze_regression(rows, history)
    regressions = print_results(rows, reg_df)

    if not args.no_plots:
        generate_visualizations(rows, history, plots_dir)

    # Also print where the artefacts live
    console.print(f"\n[bold]Artifacts in {run_dir}:[/bold]")

    def tree(path: Path, prefix: str = "") -> None:
        entries = sorted(path.iterdir(), key=lambda p: (p.is_file(), p.name.lower()))
        for idx, entry in enumerate(entries):
            is_last = idx == len(entries) - 1
            connector = "└── " if is_last else "├── "
            console.print(f"{prefix}{connector}{entry.name}")
            if entry.is_dir():
                tree(entry, prefix + ("    " if is_last else "│   "))

    tree(run_dir)

    return 1 if regressions > 0 else 0


if __name__ == "__main__":
    sys.exit(main())
