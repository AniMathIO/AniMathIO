#!/usr/bin/env python3
"""Reproducible unit-circle explainer; see ../README.md."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import BLUE, MINT, heading, main, motion, projection, ticks


def scene(b, steps):
    heading(
        b, "02", "A circle becomes a wave", "Sine is the height of a rotating point."
    )
    start, end = steps[0], steps[-1]
    center = (0.5, 0.36) if b.vertical else (0.22, 0.5)
    radius = min(b.width, b.height) * 0.18
    cx, cy = center
    rx, ry = radius / b.width, radius / b.height
    b.line(
        "Circle horizontal axis",
        [(cx - rx * 1.12, cy), (cx + rx * 1.12, cy)],
        [46, 64, 88, 160],
        stroke_width=0.0015,
        start=start,
        end=end,
    )
    b.line(
        "Circle vertical axis",
        [(cx, cy - ry * 1.12), (cx, cy + ry * 1.12)],
        [46, 64, 88, 160],
        stroke_width=0.0015,
        start=start,
        end=end,
    )
    b.text("1", cx - 0.04, cy - ry, 0.018, BLUE, start=start, end=end)
    b.text("-1", cx - 0.04, cy + ry, 0.018, BLUE, start=start, end=end)
    values = motion(b, "Unit", center, radius, start, end)
    rect = (0.5, 0.58, 0.8, 0.14) if b.vertical else (0.69, 0.5, 0.48, 0.48)
    b.graph("sin(2*x)", rect, start, end, MINT, end - start)
    projection(b, values, rect, center, radius, start, end)
    mx, my = (0.5, 0.71) if b.vertical else (0.69, 0.82)
    left, track = b.math(r"y=\sin\theta", mx, my, end=steps[2], size=64)
    right, _ = b.math(
        r"\sin(\theta+2\pi)=\sin\theta",
        mx,
        my,
        start=steps[2],
        size=43,
        track=track,
        reveal=False,
    )
    b.commands.append(
        {
            "AddTransition": {
                "track": track,
                "transition": dict(
                    id=2,
                    kind="CrossDissolve",
                    duration=ticks(0.5),
                    **{"from": left, "to": right},
                ),
            }
        }
    )
    x, y, w, h = rect
    for index, label in enumerate(["0", "π", "2π", "3π", "4π"]):
        b.text(label, x - w / 2 + w * index / 4, y + h / 2 + 0.025, 0.018, BLUE)


if __name__ == "__main__":
    main("unit-circle")
