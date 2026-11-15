"""Aggregate raw benchmark rows into results.json and results.md (median + range).

Used by scripts/bench-v1-v2.sh after the scenario runner finishes. Reads:
  raw/<scenario>.json   rows: {scenario, app, run, load1, ts, ...metrics}
  raw/machine.json, raw/versions.json, raw/sizes.json, raw/sizes_v1.json,
  raw/v1_release_assets.json, raw/authoring.json
Writes:
  results.json  machine facts + every raw row + per-scenario medians/ranges
  results.md    one table a human (and the blog post) can quote
"""

from __future__ import annotations

import argparse
import json
import re
import statistics
from pathlib import Path

KB = 1024

# scenario -> metric key -> (row label, unit, decimals)
SCENARIOS: dict[str, list[tuple[str, str, str, int]]] = {
    "cold_start": [("usable_seconds", "cold start to usable window", "s", 2)],
    "load_project_10": [("load_seconds", "project load, 10 s scene open", "s", 2)],
    "load_project_medium": [("load_seconds", "project load, 60-element project", "s", 2)],
    "export_10": [("export_seconds", "export, 10 s 1080p30 MP4", "s", 2)],
    "export_60": [("export_seconds", "export, 60 s 1080p30 MP4", "s", 2)],
    "seek_latency": [
        ("p50_ms", "seek round-trip p50", "ms", 1),
        ("p95_ms", "seek round-trip p95", "ms", 1),
    ],
}
MEM_ROWS: dict[str, list[tuple[str, str]]] = {
    # scenario -> [(json metric, table label)]
    "load_project_10": [("idle_pss_median_kb", "idle memory, 10 s scene open (PSS median)")],
    "load_project_medium": [("idle_pss_median_kb", "idle memory, 60-element project (PSS median)")],
    "export_10": [("peak_pss_kb", "peak memory during 10 s export (PSS max)")],
    "export_60": [("peak_pss_kb", "peak memory during 60 s export (PSS max)")],
}


def load_rows(raw: Path, scenario: str) -> list[dict]:
    path = raw / f"{scenario}.json"
    if not path.exists():
        return []
    try:
        return json.loads(path.read_text())
    except json.JSONDecodeError:
        return []


def fmt_cell(values: list[float], digits: int = 2) -> str:
    if not values:
        return "n/a"
    med = statistics.median(values)
    low, high = min(values), max(values)
    if len(values) == 1 or low == high:
        return f"{med:.{digits}f}"
    return f"{med:.{digits}f} ({low:.{digits}f}–{high:.{digits}f})"


def metric_stats(rows: list[dict], metric: str) -> dict:
    by_app: dict[str, list[float]] = {}
    for row in rows:
        if row.get(metric) is not None:
            by_app.setdefault(row["app"], []).append(float(row[metric]))
    return {
        app: {
            "median": statistics.median(vals) if vals else None,
            "min": min(vals) if vals else None,
            "max": max(vals) if vals else None,
            "runs": len(vals),
        }
        for app, vals in by_app.items()
    }


def ki_range_to_mib(cell: str) -> str:
    """Convert a 'median (low–high)' KiB cell to MiB for the table."""

    def repl(match: re.Match) -> str:
        return f"{float(match.group(1)) / KB:.0f}"

    return re.sub(r"(\d+(?:\.\d+)?)", repl, cell)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    out = Path(args.out)
    raw = out / "raw"

    def read(name: str) -> dict:
        path = raw / name
        return json.loads(path.read_text()) if path.exists() else {}

    machine = read("machine.json")
    versions = read("versions.json")
    sizes_v2 = read("sizes.json")
    sizes_v1 = read("sizes_v1.json")
    release_assets = read("v1_release_assets.json")
    authoring = read("authoring.json")

    results: dict = {
        "machine": machine,
        "versions": versions,
        "sizes": {
            "v2_on_disk": sizes_v2.get("v2", {}),
            "v1_artifacts": sizes_v1,
            "v1_release_downloads": release_assets,
        },
        "authoring": authoring,
        "scenarios": {},
    }

    table_rows: list[tuple[str, str, str, str]] = []
    for scenario, metrics in SCENARIOS.items():
        rows = load_rows(raw, scenario)
        if not rows:
            continue
        entry: dict = {"metrics": {}, "memory": {}, "rows": rows}
        for metric, label, unit, digits in metrics:
            stats = metric_stats(rows, metric)
            cell = {
                app: fmt_cell([row[metric] for row in rows if row.get(metric) is not None and row["app"] == app], digits)
                for app in stats
            }
            entry["metrics"][metric] = {"label": label, "unit": unit, "cell": cell, "stats": stats}
            table_rows.append((label, unit, cell.get("v1", "n/a"), cell.get("v2", "n/a")))
        for metric, label in MEM_ROWS.get(scenario, []):
            stats = metric_stats(rows, metric)
            cell = {app: fmt_cell([row[metric] for row in rows if row.get(metric) is not None and row["app"] == app], 0) for app in stats}
            entry["memory"][metric] = {"label": label, "cell_kib": cell, "stats": stats}
            table_rows.append((label, "MiB", ki_range_to_mib(cell.get("v1", "n/a")), ki_range_to_mib(cell.get("v2", "n/a"))))
        if scenario.startswith("export"):
            stats = metric_stats(rows, "output_bytes")
            cell = {
                app: fmt_cell([row["output_bytes"] / KB for row in rows if row.get("output_bytes") is not None and row["app"] == app], 0)
                for app in stats
            }
            entry["output_bytes"] = {"cell": cell, "stats": stats}
            duration = "60 s" if scenario.endswith("60") else "10 s"
            table_rows.append((f"exported MP4 size, {duration} project", "KiB", cell.get("v1", "n/a"), cell.get("v2", "n/a")))
        results["scenarios"][scenario] = entry

    (out / "results.json").write_text(json.dumps(results, indent=1))

    # ------------------------------------------------------------------ markdown
    lines: list[str] = []
    v1v, v2v = versions.get("v1", {}), versions.get("v2", {})
    lines.append("# AniMathIO v1 vs v2 benchmark results")
    lines.append("")
    lines.append(
        f"v1 {v1v.get('version')} (Electron {v1v.get('electron')}, commit `{v1v.get('commit')}`)"
        f" vs v2 {v2v.get('version')} (commit `{v2v.get('commit')}`)."
    )
    if machine:
        lines.append("")
        lines.append(
            f"**Machine:** {machine.get('cpu')}, {machine.get('cores')} cores, {machine.get('memory_gib')} GiB RAM, "
            f"{machine.get('distro')}, kernel {machine.get('kernel')}. **Display:** {machine.get('display')}. **Audio:** {machine.get('audio')}."
        )
    lines.append("")
    lines.append("Every value is the **median of N runs (min–max)**; N and the load average each run saw are in `results.json`. All GUI numbers are Xvfb (software rendering), and every run was gated on a 1-minute load average below the threshold recorded in the raw rows.")
    lines.append("")
    lines.append("| Metric | Unit | v1 (Electron/TypeScript) | v2 (Rust) |")
    lines.append("| --- | --- | --- | --- |")
    for label, unit, v1, v2 in table_rows:
        lines.append(f"| {label} | {unit} | {v1} | {v2} |")
    lines.append("")
    lines.append("## Sizes")
    lines.append("")
    if release_assets.get("assets"):
        lines.append(
            f"Published v1 {release_assets.get('tag')} download sizes: "
            + ", ".join(f"{a['name']} {a['size'] / 1024 / 1024:.0f} MiB" for a in release_assets["assets"][:6])
            + "."
        )
    if v1s := (sizes_v1 or {}):
        parts = []
        if "deb" in v1s:
            parts.append(f".deb {v1s['deb']['bytes'] / KB / KB:.0f} MiB")
        if "appimage" in v1s:
            parts.append(f"AppImage {v1s['appimage']['bytes'] / KB / KB:.0f} MiB")
        if "unpacked_dir" in v1s:
            parts.append(f"on disk (linux-unpacked) {v1s['unpacked_dir']['bytes'] / KB / KB:.0f} MiB")
        lines.append(f"- v1 artifacts (built here from origin/main): {', '.join(parts)}.")
    if v2s := sizes_v2.get("v2", {}):
        lines.append(
            f"- v2 on disk (from-source install): editor {v2s.get('animathio_editor_bytes', 0) / KB / KB:.0f} MiB"
            f" + CLI {v2s.get('animathio_cli_bytes', 0) / KB / KB:.0f} MiB"
            f" + 8 first-party plugin bundles {v2s.get('plugins_bytes', 0) / KB / KB:.1f} MiB"
            f" = **{v2s.get('on_disk_total_bytes', 0) / KB / KB:.0f} MiB**."
        )
    (out / "results.md").write_text("\n".join(lines) + "\n")
    print(f"wrote {out}/results.json and {out}/results.md")


if __name__ == "__main__":
    main()
