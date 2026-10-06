import type { RendererBridge } from "./bridge";

/** Keep the normal deadline short, but allow real-time capture and conversion. */
export async function callEditorTool(bridge: RendererBridge, name: string, args: Record<string, unknown>) {
  if (name !== "export_video") return bridge.call(name, args);
  const state = await bridge.call("get_project_state", {}) as { maxTimeMs?: number };
  const duration = state?.maxTimeMs;
  if (typeof duration !== "number" || !Number.isFinite(duration) || duration <= 0) {
    throw new Error("The project must have a finite, positive duration before exporting video.");
  }
  // Three times the recording duration plus five minutes for FFmpeg startup,
  // conversion, IPC transfer and disk I/O. Node timers have a signed 32-bit cap.
  const timeoutMs = Math.min(2_147_483_647, Math.ceil(duration * 3 + 300_000));
  return bridge.call(name, args, timeoutMs);
}
