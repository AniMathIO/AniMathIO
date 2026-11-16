"""Timed v1 vs v2 benchmark scenarios. Invoked by scripts/bench-v1-v2.sh inside
scripts/virtual-display.sh, so everything here already runs on a private Xvfb
display with audio pointed at nonexistent sound servers.

Scenarios (each run N times, interleaved, gated on a low load average; every row
records the load it ran under):

  cold_start          launch to the automation interface answering (v1: MCP
                      get_project_state after the renderer mounts; v2: control
                      project.get once the render loop serves it)
  load_project_10     launch with the 10 s scene project open, until its elements
                      are visible; idle RSS is sampled in the same session
  load_project_medium same for the 60-element, 60 s "medium" project
  export_10/60        full-project MP4 export wall time; peak process-tree PSS is
                      sampled concurrently (v1 exports record in real time)
  seek_latency        playhead round-trip latency through each app's automation
                      interface (Xvfb software rendering; not native-compositor paint)

Untimed setup: the v1 scenes are authored through v1's own MCP server and saved,
and one untimed export warms v1's ffmpeg.wasm core (fetched from unpkg on first
MP4 export) and v2's plugin compile cache, so timed exports measure steady state.
"""

from __future__ import annotations

import argparse
import json
import os
import secrets
import signal
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import procs  # noqa: E402


def now_ms() -> int:
    return time.time_ns() // 1_000_000


def load1() -> float:
    return float(Path("/proc/loadavg").read_text().split()[0])


class Bench:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.raw = Path(args.out) / "raw"
        self.raw.mkdir(parents=True, exist_ok=True)
        self.projects = Path(args.out) / "projects"
        self.exports = Path(args.out) / "exports"
        self.exports.mkdir(parents=True, exist_ok=True)
        # AF_UNIX paths are capped at 108 bytes, and bench output dirs can be
        # deep, so keep the control-socket runtime dir short and private.
        runtime_base = Path(os.environ.get("ANIMATHIO_BENCH_RUNTIME_BASE", str(Path.home() / "tmp" / "animathio-bench")))
        self.runtime = runtime_base / f"xdg-{os.getpid()}"
        self.runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
        self.v1_home = Path(args.out) / "v1home"
        self.v2_home = Path(args.out) / "v2home"
        for p in (self.v1_home, self.v2_home):
            p.mkdir(mode=0o700, parents=True, exist_ok=True)
        # The control socket and token file live under `$XDG_RUNTIME_DIR/animathio`.
        self.control_sock = self.runtime / "animathio" / "animathio-control.sock"
        self.control_token = self.runtime / "animathio" / "animathio-control.token"
        self.v1_port = self.free_port()
        self.v1_token = secrets.token_hex(24)
        self.v1_flags: list[str] = []
        self.proc: subprocess.Popen | None = None

    @staticmethod
    def free_port() -> int:
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            return s.getsockname()[1]

    # ------------------------------------------------------------------ recording

    def record(self, scenario: str, app: str, idx: int, extra: dict) -> None:
        row = {"scenario": scenario, "app": app, "run": idx, "load1": load1(), "ts": time.time()}
        row.update(extra)
        path = self.raw / f"{scenario}.json"
        rows: list = []
        if path.exists():
            try:
                rows = json.loads(path.read_text())
            except json.JSONDecodeError:
                rows = []
        rows.append(row)
        path.write_text(json.dumps(rows, indent=1))

    # ------------------------------------------------------------------ processes

    def launch_v1(self, project: Path | None = None) -> subprocess.Popen:
        argv = [self.args.v1_bin, *self.v1_flags]
        if project is not None:
            argv.append(str(project))
        env = dict(os.environ)
        # Agent harnesses set ELECTRON_RUN_AS_NODE=1, which turns the Electron
        # binary into plain Node; the app needs it unset to open a window.
        env.pop("ELECTRON_RUN_AS_NODE", None)
        env.pop("NODE_OPTIONS", None)
        env["HOME"] = str(self.v1_home)
        env.pop("ANIMATHIO_PLUGIN_DIR", None)
        log = open(self.raw / "v1-app.log", "ab")
        return subprocess.Popen(argv, env=env, stdout=log, stderr=log, start_new_session=True)

    def launch_v2(self, project: Path | None = None) -> subprocess.Popen:
        self.clear_control()
        argv = [self.args.v2_bin, "--control"]
        if project is not None:
            argv.append(str(project))
        env = dict(os.environ)
        env["HOME"] = str(self.v2_home)
        env["ANIMATHIO_PLUGIN_DIR"] = self.args.plugins
        env["XDG_RUNTIME_DIR"] = str(self.runtime)
        log = open(self.raw / "v2-app.log", "ab")
        return subprocess.Popen(argv, env=env, stdout=log, stderr=log, start_new_session=True)

    def launch_v2_cli(self, argv: list[str]) -> subprocess.Popen:
        env = dict(os.environ)
        env["HOME"] = str(self.v2_home)
        env["ANIMATHIO_PLUGIN_DIR"] = self.args.plugins
        env.pop("DISPLAY", None)
        log = open(self.raw / "v2-cli.log", "ab")
        return subprocess.Popen([self.args.v2_cli, *argv], env=env, stdout=log, stderr=log, start_new_session=True)

    @staticmethod
    def stop(proc: subprocess.Popen | None) -> None:
        if proc is None or proc.poll() is not None:
            return
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            return
        for _ in range(50):
            if proc.poll() is not None:
                return
            time.sleep(0.1)
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

    def clear_control(self) -> None:
        for p in (self.control_sock, self.control_token):
            try:
                p.unlink()
            except FileNotFoundError:
                pass
            try:
                p.parent.rmdir()
            except OSError:
                pass

    # ------------------------------------------------------------------ automation clients

    def driver(self, argv: list[str], timeout: float = 300.0) -> dict:
        cmd = [
            "node",
            str(Path(__file__).parent / "v1_driver.mjs"),
            *argv,
            "--v1", self.args.v1_src,
            "--port", str(self.v1_port),
            "--token", self.v1_token,
        ]
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        if result.returncode != 0:
            raise RuntimeError(f"v1_driver {argv[0]} failed ({result.returncode}): {result.stderr[-2000:]}")
        try:
            return json.loads(result.stdout.strip().splitlines()[-1])
        except (json.JSONDecodeError, IndexError) as error:
            raise RuntimeError(f"v1_driver printed no JSON: {result.stdout[-500:]}") from error

    def ctrl(self, argv: list[str], timeout: float = 300.0) -> dict:
        cmd = [
            "python3",
            str(Path(__file__).parent / "control_client.py"),
            "--sock", str(self.control_sock),
            "--token", str(self.control_token),
            *argv,
        ]
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env={**os.environ, "XDG_RUNTIME_DIR": str(self.runtime)})
        if result.returncode != 0:
            raise RuntimeError(f"control_client {argv[0]} failed: {result.stderr[-2000:]}")
        return json.loads(result.stdout.strip().splitlines()[-1])

    # ------------------------------------------------------------------ load gate

    def wait_for_load(self) -> float:
        waited = 0.0
        deadline = time.monotonic() + self.args.max_wait_min * 60
        while True:
            if load1() <= self.args.max_load:
                return waited
            if time.monotonic() > deadline:
                raise RuntimeError(f"load {load1()} exceeds {self.args.max_load}; refusing measurement")
            if waited == 0.0:
                print(f"[bench] load average {load1()} > {self.args.max_load}; waiting for an idle machine", flush=True)
            time.sleep(30)
            waited += 30.0


# --------------------------------------------------------------------- scenario steps


def v1_wait_usable(bench: Bench, want: int = 0, timeout_ms: float = 180_000) -> dict:
    return bench.driver(["wait", "--want", str(want), "--timeout", str(int(timeout_ms))])


def v2_wait_usable(bench: Bench, clips: int = 0, timeout_ms: float = 180_000) -> dict:
    return bench.ctrl(["wait", "--clips", str(clips), "--timeout-ms", str(int(timeout_ms))], timeout=timeout_ms / 1000 + 30)


def preflight_v1(bench: Bench) -> None:
    """Seed v1's electron-store config in the benchmark HOME so the opt-in MCP
    server comes up on launch, then verify it answers. electron-store reads
    `<userData>/config.json`; on Linux userData is `~/.config/<app name>` (the
    packaged app uses the lowercase package name)."""
    print("[bench] v1 preflight: enabling the opt-in MCP server in a private HOME", flush=True)

    def seed_all() -> None:
        cfg = json.dumps({"mcpServerEnabled": True, "mcpServerPort": bench.v1_port, "mcpServerToken": bench.v1_token})
        for name in ("animathio", "AniMathIO"):
            d = bench.v1_home / ".config" / name
            d.mkdir(parents=True, exist_ok=True)
            (d / "config.json").write_text(cfg)

    def discover_and_seed() -> bool:
        """Find Chromium userData dirs created by the first launch and seed them too."""
        seeded = False
        cfg_dir = bench.v1_home / ".config"
        cfg = json.dumps({"mcpServerEnabled": True, "mcpServerPort": bench.v1_port, "mcpServerToken": bench.v1_token})
        for d in cfg_dir.iterdir() if cfg_dir.exists() else []:
            if d.is_dir() and (d / "GPUCache").exists() and not (d / "config.json").exists():
                (d / "config.json").write_text(cfg)
                seeded = True
        return seeded

    seed_all()
    for attempt in range(3):
        proc = bench.launch_v1()
        try:
            try:
                v1_wait_usable(bench, timeout_ms=90_000)
                print(f"[bench] v1 MCP server answering (port {bench.v1_port}, flags={bench.v1_flags})", flush=True)
                return
            except Exception as error:
                if attempt == 0 and discover_and_seed():
                    print(f"[bench] re-seeding discovered config dirs: {error}", flush=True)
                    continue
                if attempt >= 1 and not bench.v1_flags:
                    print(f"[bench] retrying v1 with --no-sandbox: {error}", flush=True)
                    bench.v1_flags = ["--no-sandbox"]
                    continue
                raise
        finally:
            Bench.stop(proc)
    raise SystemExit("v1 preflight failed: MCP server never answered (see raw/v1-app.log)")


def author_v1_scene(bench: Bench, steps_file: Path, project_out: str) -> dict:
    """Open the seed project, run the MCP steps, save (untimed)."""
    seed = Path(bench.args.out) / "fixtures" / "v1-seed.animathio"
    proc = bench.launch_v1(seed)
    try:
        state = v1_wait_usable(bench)
        result = bench.driver(["call", "--script", str(steps_file)], timeout=600)
        return {"project": project_out, "authored_from_elements": state["elements"], "steps": len(result)}
    finally:
        Bench.stop(proc)


def run_timed_export_v1(bench: Bench, project: Path, out_mp4: Path, idx: int, scenario: str) -> None:
    bench.wait_for_load()
    proc = bench.launch_v1(project)
    try:
        v1_wait_usable(bench)
        state = bench.driver(["wait", "--want", "0", "--timeout", "60000"])
        duration_ms = state["maxTimeMs"]
        timeout = duration_ms * 4 + 420_000
        samples: list[dict] = []
        stop = threading.Event()

        def sample_loop() -> None:
            pss_peak = 0
            rss_peak = 0
            mode = "pss"
            n = 0
            while not stop.is_set() and proc.poll() is None:
                one = procs.sample_for(proc.pid, seconds=0.25, hz=8.0)
                n += one["samples"]
                mode = one["mode"]
                if one["pss_max_kb"] is not None:
                    pss_peak = max(pss_peak, one["pss_max_kb"])
                if one["rss_max_kb"] is not None:
                    rss_peak = max(rss_peak, one["rss_max_kb"])
            samples.append(
                {
                    "mode": mode,
                    "samples": n,
                    "pss_max_kb": pss_peak or None,
                    "rss_max_kb": rss_peak or None,
                }
            )

        thread = threading.Thread(target=sample_loop, daemon=True)
        thread.start()
        started = now_ms()
        bench.driver(["export", "--path", str(out_mp4), "--timeout", str(int(timeout))], timeout=timeout / 1000 + 60)
        finished = now_ms()
        stop.set()
        thread.join(timeout=10)
        mem = samples[-1] if samples else {}
        size = out_mp4.stat().st_size if out_mp4.exists() else 0
        bench.record(scenario, "v1", idx, {
            "export_seconds": round((finished - started) / 1000, 3),
            "output_bytes": size,
            "probe": validate_mp4(out_mp4, 60 if scenario.endswith("60") else 10),
            "duration_ms": duration_ms,
            "peak_pss_kb": mem.get("pss_max_kb"),
            "peak_rss_kb": mem.get("rss_max_kb"),
            "mem_mode": mem.get("mode"),
        })
    finally:
        Bench.stop(proc)


def run_timed_export_v2(bench: Bench, project: Path, out_mp4: Path, idx: int, scenario: str) -> None:
    bench.wait_for_load()
    bench.clear_control()
    proc = bench.launch_v2_cli(["export", str(project), "-o", str(out_mp4), "--overwrite"])
    stop = threading.Event()
    samples: list[dict] = []

    def sample_loop() -> None:
        pss_peak = 0
        rss_peak = 0
        mode = "pss"
        n = 0
        while not stop.is_set() and proc.poll() is None:
            one = procs.sample_for(proc.pid, seconds=0.25, hz=8.0)
            n += one["samples"]
            mode = one["mode"]
            if one["pss_max_kb"] is not None:
                pss_peak = max(pss_peak, one["pss_max_kb"])
            if one["rss_max_kb"] is not None:
                rss_peak = max(rss_peak, one["rss_max_kb"])
        samples.append({"mode": mode, "samples": n, "pss_max_kb": pss_peak or None, "rss_max_kb": rss_peak or None})

    thread = threading.Thread(target=sample_loop, daemon=True)
    thread.start()
    started = now_ms()
    try:
        proc.wait(timeout=900)
    except subprocess.TimeoutExpired:
        Bench.stop(proc)
        raise SystemExit(f"v2 export timed out: {scenario}")
    finished = now_ms()
    stop.set()
    thread.join(timeout=10)
    if proc.returncode != 0:
        raise SystemExit(f"v2 export failed ({proc.returncode}); see raw/v2-cli.log")
    mem = samples[-1] if samples else {}
    bench.record(scenario, "v2", idx, {
        "export_seconds": round((finished - started) / 1000, 3),
        "output_bytes": out_mp4.stat().st_size,
        "probe": validate_mp4(out_mp4, 60 if scenario.endswith("60") else 10),
        "peak_pss_kb": mem.get("pss_max_kb"),
        "peak_rss_kb": mem.get("rss_max_kb"),
        "mem_mode": mem.get("mode"),
    })


def validate_mp4(path: Path, seconds: int) -> dict:
    result = subprocess.run([
        "ffprobe", "-v", "error", "-show_entries",
        "stream=codec_name,width,height,r_frame_rate,nb_frames:format=duration",
        "-of", "json", str(path),
    ], check=True, capture_output=True, text=True)
    probe = json.loads(result.stdout)
    video = next(s for s in probe["streams"] if s.get("width"))
    if (video["codec_name"], video["width"], video["height"], video["r_frame_rate"]) != ("h264", 1920, 1080, "30/1"):
        raise RuntimeError(f"unexpected export: {probe}")
    if abs(float(probe["format"]["duration"]) - seconds) > 0.2:
        raise RuntimeError(f"wrong export duration: {probe}")
    return probe


def cold_start(bench: Bench, app: str, idx: int) -> None:
    waited = bench.wait_for_load()
    launch = now_ms()
    proc = bench.launch_v1() if app == "v1" else bench.launch_v2()
    try:
        if app == "v1":
            state = v1_wait_usable(bench)
            ready = state["loaded"]
            bench.record("cold_start", app, idx, {
                "gate_wait_s": waited,
                "usable_seconds": round((ready - launch) / 1000, 3),
                "automation_ready_seconds": round((state["mcpReady"] - launch) / 1000, 3),
            })
        else:
            state = v2_wait_usable(bench)
            ready = state["ready_at"]
            bench.record("cold_start", app, idx, {
                "gate_wait_s": waited,
                "usable_seconds": round((ready - launch) / 1000, 3),
                "automation_ready_seconds": round((state["connected_at"] - launch) / 1000, 3),
            })
    finally:
        Bench.stop(proc)


def load_and_idle(bench: Bench, app: str, scenario: str, project: Path, want: int, idx: int) -> None:
    waited = bench.wait_for_load()
    launch = now_ms()
    proc = bench.launch_v1(project) if app == "v1" else bench.launch_v2(project)
    try:
        if app == "v1":
            state = v1_wait_usable(bench, want=want)
            ready = state["loaded"]
        else:
            state = v2_wait_usable(bench, clips=want)
            ready = state["ready_at"]
        load_seconds = round((ready - launch) / 1000, 3)
        mem = procs.sample_for(proc.pid, seconds=8.0, hz=4.0, skip=1.0)
        bench.record(scenario, app, idx, {
            "gate_wait_s": waited,
            "load_seconds": load_seconds,
            "elements": state.get("elements") if app == "v1" else state.get("clips"),
            "idle_pss_median_kb": mem.get("pss_median_kb"),
            "idle_rss_median_kb": mem.get("rss_median_kb"),
            "idle_pss_max_kb": mem.get("pss_max_kb"),
            "mem_mode": mem.get("mode"),
        })
    finally:
        Bench.stop(proc)


def seek_latency(bench: Bench, app: str, project: Path, idx: int) -> None:
    bench.wait_for_load()
    proc = bench.launch_v1(project) if app == "v1" else bench.launch_v2(project)
    try:
        if app == "v1":
            v1_wait_usable(bench)
            result = bench.driver(["seek", "--seconds", "10", "--hz", "60"], timeout=120)
        else:
            v2_wait_usable(bench, clips=5)
            result = bench.ctrl(["seek-bench", "--seconds", "10", "--hz", "60"], timeout=120)
        bench.record("seek_latency", app, idx, {**result, "hz": 60, "seconds": 10})
    finally:
        Bench.stop(proc)


# --------------------------------------------------------------------- main


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", required=True)
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--v1-bin", required=True)
    ap.add_argument("--v1-src", required=True)
    ap.add_argument("--v2-bin", required=True)
    ap.add_argument("--v2-cli", required=True)
    ap.add_argument("--plugins", required=True)
    ap.add_argument("--max-load", type=float, default=4.0)
    ap.add_argument("--max-wait-min", type=float, default=180.0)
    args = ap.parse_args()
    bench = Bench(args)
    out = Path(args.out)
    fixtures = out / "fixtures"
    projects = out / "projects"

    skip_seek = os.environ.get("ANIMATHIO_BENCH_SKIP_SEEK") == "1"
    skip_medium = os.environ.get("ANIMATHIO_BENCH_SKIP_MEDIUM") == "1"

    scene10 = projects / "scene-10.animathio"
    scene60 = projects / "scene-60.animathio"
    medium = projects / "medium.animathio"
    scene10_amio = fixtures / "scene-10.amio"
    scene60_amio = fixtures / "scene-60.amio"
    medium_amio = fixtures / "medium.amio"
    for p in (bench.raw.parent / "fixtures" / "v1-seed.animathio", scene10_amio, scene60_amio, medium_amio,
              fixtures / "steps-v1-scene-10.json", fixtures / "steps-v1-scene-60.json", fixtures / "steps-v1-medium.json"):
        if not p.exists():
            raise SystemExit(f"missing fixture: {p} (run the setup phase first)")

    # ---- untimed setup -------------------------------------------------------
    preflight_v1(bench)

    print("[bench] authoring v1 scenes over MCP (untimed)", flush=True)
    info = {}
    info["scene10"] = author_v1_scene(bench, fixtures / "steps-v1-scene-10.json", "scene-10.animathio")
    info["scene60"] = author_v1_scene(bench, fixtures / "steps-v1-scene-60.json", "scene-60.animathio")
    info["medium"] = author_v1_scene(bench, fixtures / "steps-v1-medium.json", "medium.animathio")
    (bench.raw / "authoring.json").write_text(json.dumps(info, indent=1))
    for name in ("scene-10.animathio", "scene-60.animathio", "medium.animathio"):
        if not (projects / name).exists():
            raise SystemExit(f"v1 did not save {name} (see raw/v1-app.log)")

    # ---- startup and load scenarios -----------------------------------------------------
    for idx in range(1, args.runs + 1):
        print(f"[bench] === run {idx}/{args.runs} ===", flush=True)
        cold_start(bench, "v1", idx)
        cold_start(bench, "v2", idx)

        load_and_idle(bench, "v1", "load_project_10", scene10, 3, idx)
        load_and_idle(bench, "v2", "load_project_10", scene10_amio, 5, idx)

        if not skip_medium:
            load_and_idle(bench, "v1", "load_project_medium", medium, 60, idx)
            load_and_idle(bench, "v2", "load_project_medium", medium_amio, 60, idx)


    print("[bench] warm-up: v1 ffmpeg.wasm core (fetched from unpkg on first MP4 export)", flush=True)
    warm = bench.launch_v1(scene10)
    try:
        v1_wait_usable(bench)
        bench.driver(
            ["export", "--path", str(bench.exports / "warmup-v1.mp4"), "--timeout", "420000"],
            timeout=480,
        )
    finally:
        Bench.stop(warm)
    if not (bench.exports / "warmup-v1.mp4").exists():
        raise SystemExit("v1 warm-up export failed (see raw/v1-app.log)")

    print("[bench] warm-up: v2 GUI + plugin compile cache", flush=True)
    warm = bench.launch_v2(scene10_amio)
    try:
        v2_wait_usable(bench, clips=5)
        time.sleep(2.0)
    finally:
        Bench.stop(warm)
    print("[bench] warm-up: v2 CLI export (plugin compile cache)", flush=True)
    warm_cli = bench.launch_v2_cli(["export", str(scene10_amio), "-o", str(bench.exports / "warmup-v2.mp4"), "--overwrite"])
    warm_cli.wait(timeout=600)
    if warm_cli.returncode != 0:
        raise SystemExit("v2 warm-up export failed (see raw/v2-cli.log)")

    for idx in range(1, args.runs + 1):
        run_timed_export_v1(bench, scene10, bench.exports / f"v1-10-run{idx}.mp4", idx, "export_10")
        run_timed_export_v2(bench, scene10_amio, bench.exports / f"v2-10-run{idx}.mp4", idx, "export_10")

        run_timed_export_v1(bench, scene60, bench.exports / f"v1-60-run{idx}.mp4", idx, "export_60")
        run_timed_export_v2(bench, scene60_amio, bench.exports / f"v2-60-run{idx}.mp4", idx, "export_60")

        if not skip_seek:
            seek_latency(bench, "v1", scene60, idx)
            seek_latency(bench, "v2", scene60_amio, idx)

    # v1 packaged sizes (deb / AppImage / unpacked tree) for the size comparison.
    sizes: dict = {}
    for pattern, key in (
        ("dist/*.deb", "deb"),
        ("dist/*.AppImage", "appimage"),
        ("dist/linux-unpacked", "unpacked_dir"),
    ):
        matches = sorted(Path(bench.args.v1_src).glob(pattern))
        if matches:
            artifact = matches[-1]
            r = subprocess.run(["du", "-sb", str(artifact)], capture_output=True, text=True)
            sizes[key] = {"path": artifact.name, "bytes": int(r.stdout.split()[0]) if r.returncode == 0 else None}
    (bench.raw / "sizes_v1.json").write_text(json.dumps(sizes, indent=1))

    print("[bench] scenarios complete", flush=True)


if __name__ == "__main__":
    main()
