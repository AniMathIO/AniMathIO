#!/usr/bin/env python3
"""Reproducible fourier explainer; see ../README.md."""

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import MINT, heading, main, motion, projection, ticks


def scene(b, steps):
    heading(b, "03", "Build a square wave", "Add odd harmonics. Watch the circles sum.")
    center = (0.5, 0.38) if b.vertical else (0.23, 0.5)
    rect = (0.5, 0.58, 0.8, 0.14) if b.vertical else (0.71, 0.52, 0.46, 0.42)
    radius = min(b.width, b.height) * 0.10
    # The ideal discontinuous target stays behind each progressively richer sum.
    b.graph("sin(x)/abs(sin(x))", rect, steps[0], steps[-1], [85, 105, 130, 160], 0.8)
    graph_track = None
    previous = None
    for i, count in enumerate([1, 2, 4]):
        start, end = steps[i], steps[i + 1]
        values = motion(b, "Epicycle", center, radius, start, end, count)
        projection(b, values, rect, center, radius, start, end)
        expr = "+".join(
            f"{4 / (math.pi * n):.10f}*sin({n}*x)" for n in range(1, count * 2, 2)
        )
        identity, graph_track = b.graph(
            expr, rect, start, end, MINT, end - start, graph_track
        )
        if previous is not None:
            b.commands.append(
                {
                    "AddTransition": {
                        "track": graph_track,
                        "transition": dict(
                            id=i + 10,
                            kind="CrossDissolve",
                            duration=ticks(0.4),
                            **{"from": previous, "to": identity},
                        ),
                    }
                }
            )
        previous = identity
        b.text(
            f"{count} odd harmonic{'s' if count > 1 else ''}",
            rect[0],
            0.675 if b.vertical else 0.285,
            0.025,
            MINT,
            start=start,
            end=end,
        )
    b.math(
        r"f(x)=\frac{4}{\pi}\left(\sin x+\frac{\sin3x}{3}+\cdots\right)",
        0.5,
        0.73 if b.vertical else 0.78,
        size=39,
    )


if __name__ == "__main__":
    main("fourier")
