#!/usr/bin/env python3
"""Reproducible pythagorean explainer; see ../README.md."""

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import BLUE, GOLD, MINT, PINK, WHITE, curve, heading, main, ticks, transform


def scene(b, steps):
    heading(
        b,
        "01",
        "The Pythagorean theorem",
        "One square. Four triangles. Two arrangements.",
    )
    # A 3-4-5 dissection: initially the uncovered central diamond is c²;
    # finally two pairs make 3x4 rectangles, leaving 3² and 4² squares.
    side = (0.58 if b.vertical else 0.50) * min(b.width, b.height)
    sx, sy = side / b.width, side / b.height
    cx, cy = (0.5, 0.44) if b.vertical else (0.32, 0.52)
    box = transform(cx, cy, sx, sy)
    b.clip(
        "Outer square",
        "graph",
        {
            "items": "\n".join(
                [
                    "segment x1=0 y1=0 x2=1 y2=0",
                    "segment x1=1 y1=0 x2=1 y2=1",
                    "segment x1=1 y1=1 x2=0 y2=1",
                    "segment x1=0 y1=1 x2=0 y2=0",
                ]
            ),
            "x_min": 0,
            "x_max": 1,
            "y_min": 0,
            "y_max": 1,
            "scale_mode": "stretch",
            "show_grid": False,
            "show_axes": False,
            "show_labels": False,
            "color": WHITE,
            "stroke_width": b.graph_stroke * 4 / 3,
        },
        placement=box,
    )
    a, d = 3 / 7, 4 / 7
    initial = [
        ((0, 0), 0),
        ((1, 0), math.pi / 2),
        ((1, 1), math.pi),
        ((0, 1), 3 * math.pi / 2),
    ]
    # Local right angle at origin. Targets tile the upper-right and lower-left rectangles.
    targets = [
        ((1, 0), math.pi / 2),
        ((a, a), -math.pi / 2),
        ((0, a), 0),
        ((a, 1), math.pi),
    ]
    # Triangle 3 has swapped legs after quarter-turn: the two remaining rectangles are a*d.
    for i, ((origin, angle), (target, end_angle)) in enumerate(zip(initial, targets)):

        def position(p):
            return cx + (p[0] - 0.5) * sx, cy + (p[1] - 0.5) * sy

        x, y = position(origin)
        tx, ty = position(target)
        # Shape coordinates centered at its right angle; rotation is in physical canvas space.
        points = [(0.5, 0.5), (0.5 + a, 0.5), (0.5, 0.5 + d)]
        animations = [
            curve(
                {"Transform": "X"}, [(0, x), (steps[1], x), (steps[2], tx)], "EaseInOut"
            ),
            curve(
                {"Transform": "Y"}, [(0, y), (steps[1], y), (steps[2], ty)], "EaseInOut"
            ),
            curve(
                {"Transform": "Rotation"},
                [(0, angle), (steps[1], angle), (steps[2], end_angle)],
                "EaseInOut",
            ),
        ]
        b.shape(
            f"Congruent triangle {i + 1}",
            points,
            [BLUE, MINT, PINK, GOLD][i],
            True,
            transform(x, y, sx, sy, angle),
            animations,
        )
    # Areas highlighted behind labels once the dissection settles.
    b.text("c²", cx, cy, 0.075, GOLD, end=steps[1])
    ax, ay = cx + (a / 2 - 0.5) * sx, cy + (a / 2 - 0.5) * sy
    dx, dy = cx + ((1 + a) / 2 - 0.5) * sx, cy + ((1 + a) / 2 - 0.5) * sy
    b.text("a²", ax, ay, 0.06, GOLD, start=steps[2])
    b.text("b²", dx, dy, 0.06, GOLD, start=steps[2])
    ex, ey = (0.5, 0.68) if b.vertical else (0.77, 0.43)
    _, track = b.math(r"(a+b)^2-4\frac{ab}{2}=c^2", ex, ey, end=steps[2], size=46)
    right, _ = b.math(
        r"a^2+b^2=c^2", ex, ey, start=steps[2], size=58, track=track, reveal=False
    )
    left = right - 2
    b.commands.append(
        {
            "AddTransition": {
                "track": track,
                "transition": dict(
                    id=1,
                    kind="CrossDissolve",
                    duration=ticks(0.6),
                    **{"from": left, "to": right},
                ),
            }
        }
    )
    b.text(
        "a = 3     b = 4     c = 5",
        ex,
        ey + (0.06 if b.vertical else 0.1),
        0.026,
        MINT,
    )


if __name__ == "__main__":
    main("pythagorean")
