"""Client for v2's control socket, used only by scripts/bench-v1-v2.sh.

Talks newline-delimited JSON-RPC 2.0 (with the token from the well-known
0600 token file) to a running `animathio --control` editor, exactly like
`crates/control/src/client.rs` does.

Commands (one JSON object on stdout each):

    control_client.py call  --method project.get [--params '{"json":..}']
    control_client.py wait  [--clips N] [--timeout-ms MS]
        Poll `project.get` until the editor answers (--clips 0: any answer).
        Prints {"connected_at", "ready_at", "clips", "duration_ticks"}.
    control_client.py seek-bench [--seconds 10] [--hz 60]
        Pseudo-random `playback.seek` round-trips through the control socket;
        prints {"samples", "p50_ms", "p95_ms", "max_ms"}.

`--sock`/`--token` override the well-known socket paths (the benchmark runs
with its own XDG_RUNTIME_DIR).
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import sys
import time


class ControlConnection:
    def __init__(self, sock_path: str, token_path: str) -> None:
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(sock_path)
        self.sock.settimeout(120)
        with open(token_path, encoding="utf-8") as fh:
            self.token = fh.read().strip()
        self.next_id = 1

    def call(self, method: str, params: object | None = None, timeout: float = 120.0) -> object:
        request = {
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": method,
            "params": params,
            "token": self.token,
        }
        self.next_id += 1
        payload = json.dumps(request, separators=(",", ":")).encode()
        if b"\n" in payload:
            raise ValueError("request must be a single JSON line")
        self.sock.settimeout(timeout)
        self.sock.sendall(payload + b"\n")
        buf = b""
        while b"\n" not in buf:
            chunk = self.sock.recv(65_536)
            if not chunk:
                raise ConnectionError("control server closed the connection")
            buf += chunk
        line, _, rest = buf.partition(b"\n")
        del rest
        response = json.loads(line)
        if "error" in response:
            raise RuntimeError(f"control error: {response['error']}")
        return response.get("result")


def clip_count(result: object) -> int:
    project = (result or {}).get("project", {}) if isinstance(result, dict) else {}
    return sum(len(track.get("clips", {})) for track in project.get("tracks", []))


def duration_ticks(result: object) -> int:
    project = (result or {}).get("project", {}) if isinstance(result, dict) else {}
    return int(project.get("duration", 0))


def paths(args: argparse.Namespace) -> tuple[str, str]:
    sock = args.sock or os.path.join(os.environ.get("XDG_RUNTIME_DIR", "/run/user/0"), "animathio-control.sock")
    token = args.token or sock.replace(".sock", ".token")
    return sock, token


def cmd_call(args: argparse.Namespace) -> None:
    sock, token = paths(args)
    deadline = time.monotonic() + args.timeout_ms / 1000.0
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            conn = ControlConnection(sock, token)
            result = conn.call(args.method, json.loads(args.params) if args.params else None)
            print(json.dumps({"result": result}))
            return
        except (FileNotFoundError, ConnectionRefusedError, ConnectionError, PermissionError) as error:
            last_error = error
            time.sleep(0.025)
    raise SystemExit(f"control socket never answered: {last_error}")


def cmd_wait(args: argparse.Namespace) -> None:
    sock, token = paths(args)
    deadline = time.monotonic() + args.timeout_ms / 1000.0
    connected_at = None
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            conn = ControlConnection(sock, token)
            connected_at = time.time_ns() // 1_000_000
            break
        except (FileNotFoundError, ConnectionRefusedError, ConnectionError, PermissionError) as error:
            last_error = error
            time.sleep(0.02)
    if connected_at is None:
        raise SystemExit(f"control socket never came up: {last_error}")
    while True:
        if time.monotonic() > deadline:
            raise SystemExit("project never became usable in time")
        try:
            conn = ControlConnection(sock, token)
            result = conn.call("project.get", None, timeout=30.0)
        except (FileNotFoundError, ConnectionRefusedError, ConnectionError, PermissionError) as error:
            last_error = error
            time.sleep(0.02)
            continue
        except (socket.timeout, RuntimeError) as error:
            last_error = error
            time.sleep(0.05)
            continue
        clips = clip_count(result)
        if clips >= args.clips:
            print(
                json.dumps(
                    {
                        "connected_at": connected_at,
                        "ready_at": time.time_ns() // 1_000_000,
                        "clips": clips,
                        "duration_ticks": duration_ticks(result),
                    }
                )
            )
            return


def cmd_seek_bench(args: argparse.Namespace) -> None:
    sock, token = paths(args)
    conn = ControlConnection(sock, token)
    state = conn.call("project.get", None)
    max_ticks = int(duration_ticks(state)) or 1
    seed = 12345

    def nxt() -> int:
        nonlocal seed
        seed = (seed * 1103515245 + 12345) % (1 << 32)
        return seed

    latencies: list[float] = []
    end = time.monotonic() + args.seconds
    hz = args.hz
    while time.monotonic() < end:
        due = time.monotonic() + 1.0 / hz
        t0 = time.perf_counter()
        conn.call("playback.seek", {"time": nxt() % max_ticks}, timeout=60.0)
        latencies.append((time.perf_counter() - t0) * 1000.0)
        remain = due - time.monotonic()
        if remain > 0:
            time.sleep(remain)
    latencies.sort()
    pct = lambda p: latencies[min(len(latencies) - 1, int(p * len(latencies)))]  # noqa: E731
    print(
        json.dumps(
            {
                "samples": len(latencies),
                "p50_ms": round(pct(0.5), 3),
                "p95_ms": round(pct(0.95), 3),
                "max_ms": round(latencies[-1], 3),
            }
        )
    )


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--sock", default=None)
    ap.add_argument("--token", default=None)
    sub = ap.add_subparsers(dest="cmd", required=True)

    call = sub.add_parser("call")
    call.add_argument("--method", required=True)
    call.add_argument("--params", default=None)
    call.add_argument("--timeout-ms", type=float, default=60_000)
    call.set_defaults(fn=cmd_call)

    wait = sub.add_parser("wait")
    wait.add_argument("--clips", type=int, default=0)
    wait.add_argument("--timeout-ms", type=float, default=180_000)
    wait.set_defaults(fn=cmd_wait)

    seek = sub.add_parser("seek-bench")
    seek.add_argument("--seconds", type=float, default=10.0)
    seek.add_argument("--hz", type=float, default=60.0)
    seek.set_defaults(fn=cmd_seek_bench)

    args = ap.parse_args()
    try:
        args.fn(args)
    except BrokenPipeError:
        sys.exit(3)


if __name__ == "__main__":
    main()
