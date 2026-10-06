import { describe, expect, it, vi } from "vitest";
import { callEditorTool } from "../../main/mcp/tool-call";
import type { RendererBridge } from "../../main/mcp/bridge";

describe("MCP export deadline", () => {
  it("uses project duration to allow recording and conversion, forwarding the arguments", async () => {
    const call = vi.fn().mockResolvedValueOnce({ maxTimeMs: 120000 }).mockResolvedValueOnce({ exported: true });
    const bridge = { call } as unknown as RendererBridge;
    const args = { path: "/tmp/scene.webm" };
    await expect(callEditorTool(bridge, "export_video", args)).resolves.toEqual({ exported: true });
    expect(call.mock.calls).toEqual([["get_project_state", {}], ["export_video", args, 660000]]);
  });
  it("retains the bridge default deadline for ordinary tools", async () => {
    const call = vi.fn().mockResolvedValue({ id: "text" });
    await callEditorTool({ call } as unknown as RendererBridge, "add_text", { text: "Hi" });
    expect(call).toHaveBeenCalledExactlyOnceWith("add_text", { text: "Hi" });
  });
  it.each([NaN, Infinity, -1, 0, undefined])("rejects invalid duration %s before starting recording", async (maxTimeMs) => {
    const call = vi.fn().mockResolvedValue({ maxTimeMs });
    await expect(callEditorTool({ call } as unknown as RendererBridge, "export_video", {})).rejects.toThrow("finite, positive duration");
    expect(call).toHaveBeenCalledTimes(1);
  });
  it("caps deadlines at Node's timer limit", async () => {
    const call = vi.fn().mockResolvedValue({ maxTimeMs: 1e12 });
    await callEditorTool({ call } as unknown as RendererBridge, "export_video", {});
    expect(call).toHaveBeenLastCalledWith("export_video", {}, 2147483647);
  });
});
