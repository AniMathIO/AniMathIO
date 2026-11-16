"""Process-tree memory sampler for the v1 vs v2 benchmark (used by scripts/bench-v1-v2.sh).

Walks /proc from a root PID through `task/<tid>/children` and sums PSS (from
`smaps_rollup`, which charges shared pages once) and RSS for every live process
in the tree. Electron runs several processes, so the tree sum is the honest
single number for v1; the v2 CLI is a single process and gets the same treatment.

    python3 procs.py --pid 1234 --seconds 8 --hz 4
    python3 procs.py --pid 1234 --seconds 8 --hz 4 --skip 1.0

Prints one JSON object on stdout. PSS falls back to RSS-only (mode "rss") on
kernels without smaps_rollup.
"""

from __future__ import annotations

import argparse
import json
import os
import statistics
import time

PAGE_KB = os.sysconf("SC_PAGE_SIZE") / 1024.0


def child_pids(pid: int) -> list[int]:
    try:
        from pathlib import Path
        children = set()
        for task in Path(f"/proc/{pid}/task").iterdir():
            try:
                children.update(int(c) for c in (task / "children").read_text().split())
            except (OSError, ValueError):
                continue
        return sorted(children)
    except (FileNotFoundError, ProcessLookupError, PermissionError, ValueError):
        return []


def tree_pids(root: int) -> list[int]:
    """The root and all descendants, by polling the children files (children may
    appear and vanish between samples; whatever exists at sample time is summed)."""
    seen: list[int] = []
    frontier = [root]
    while frontier:
        nxt: list[int] = []
        for pid in frontier:
            seen.append(pid)
            nxt.extend(child_pids(pid))
        frontier = nxt
    return seen


def sample_once(root: int) -> tuple[int | None, int | None, bool]:
    """One summed snapshot: (pss_kb | None, rss_kb | None, have_pss)."""
    total_pss = 0
    total_rss = 0
    pss_seen = False
    found = False
    for pid in tree_pids(root):
        rss_kb = None
        try:
            with open(f"/proc/{pid}/statm") as fh:
                rss_pages = int(fh.read().split()[1])
            rss_kb = round(rss_pages * PAGE_KB)
        except (FileNotFoundError, ProcessLookupError, PermissionError, ValueError, IndexError):
            rss_kb = None
        pss_kb = None
        try:
            with open(f"/proc/{pid}/smaps_rollup") as fh:
                for line in fh:
                    if line.startswith("Pss:"):
                        pss_kb = int(line.split()[1])
                        break
        except (FileNotFoundError, ProcessLookupError, PermissionError, ValueError):
            pss_kb = None
        if rss_kb is None and pss_kb is None:
            continue  # process vanished mid-walk
        found = True
        total_pss += pss_kb if pss_kb is not None else (rss_kb or 0)
        total_rss += rss_kb if rss_kb is not None else (pss_kb or 0)
        if pss_kb is not None:
            pss_seen = True
    return (total_pss if found else None, total_rss if found else None, pss_seen)


def sample_for(pid: int, seconds: float, hz: float, skip: float = 0.0) -> dict:
    """Sample the process tree for `seconds` at `hz` and return the summary dict."""
    if skip > 0:
        time.sleep(skip)
    pss_samples: list[int] = []
    rss_samples: list[int] = []
    have_pss = False
    interval = 1.0 / max(hz, 0.1)
    iterations = max(1, round(seconds * hz))
    for _ in range(iterations):
        started = time.monotonic()
        pss, rss, pss_ok = sample_once(pid)
        if pss is not None:
            pss_samples.append(pss)
        if rss is not None:
            rss_samples.append(rss)
        have_pss = have_pss or pss_ok
        remain = interval - (time.monotonic() - started)
        if remain > 0:
            time.sleep(remain)

    def med(values: list[int]) -> int | None:
        return round(statistics.median(values)) if values else None

    def mx(values: list[int]) -> int | None:
        return max(values) if values else None

    return {
        "mode": "pss" if have_pss else "rss",
        "samples": max(len(pss_samples), len(rss_samples)),
        "pss_median_kb": med(pss_samples) if have_pss else None,
        "pss_max_kb": mx(pss_samples) if have_pss else None,
        "rss_median_kb": med(rss_samples),
        "rss_max_kb": mx(rss_samples),
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--pid", type=int, required=True)
    ap.add_argument("--seconds", type=float, default=8.0)
    ap.add_argument("--hz", type=float, default=4.0)
    ap.add_argument("--skip", type=float, default=0.0, help="Seconds to wait before sampling.")
    args = ap.parse_args()
    print(json.dumps(sample_for(args.pid, args.seconds, args.hz, args.skip)))


if __name__ == "__main__":
    main()
