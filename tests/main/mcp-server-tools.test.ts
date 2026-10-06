import { describe, expect, it, vi } from "vitest";
import { CallToolRequestSchema } from "@modelcontextprotocol/sdk/types.js";
import { AniMathIOMcpServer } from "../../main/mcp/server";
import type { RendererBridge } from "../../main/mcp/bridge";

vi.mock("@modelcontextprotocol/sdk/server/index.js", () => ({
  Server: class {
    handlers = new Map();
    setRequestHandler(schema: unknown, handler: unknown) { this.handlers.set(schema, handler); }
  },
}));
vi.mock("@modelcontextprotocol/sdk/server/streamableHttp.js", () => ({
  StreamableHTTPServerTransport: class {},
}));

function handler(call: ReturnType<typeof vi.fn>) {
  const server = new AniMathIOMcpServer({ call } as unknown as RendererBridge);
  // Exercise the actual SDK handler registration without opening a socket.
  const instance = (server as any).buildInstance();
  return instance.mcp.handlers.get(CallToolRequestSchema) as (request: any) => Promise<any>;
}

describe("MCP server tool requests", () => {
  it("passes a project-derived export deadline through the actual request handler", async () => {
    const call = vi.fn().mockResolvedValueOnce({ maxTimeMs: 45000 }).mockResolvedValueOnce({ exported: true });
    const args = { path: "/tmp/scene.webm" };
    const response = await handler(call)({ params: { name: "export_video", arguments: args } });
    expect(call.mock.calls).toEqual([["get_project_state", {}], ["export_video", args, 435000]]);
    expect(JSON.parse(response.content[0].text)).toEqual({ exported: true });
  });
  it("forwards media arguments using the normal deadline", async () => {
    const call = vi.fn().mockResolvedValue({ ids: ["image-1"], type: "image" });
    const args = { path: "/tmp/image.png" };
    const response = await handler(call)({ params: { name: "add_media", arguments: args } });
    expect(call).toHaveBeenCalledExactlyOnceWith("add_media", args);
    expect(JSON.parse(response.content[0].text)).toEqual({ ids: ["image-1"], type: "image" });
  });
  it("returns export failures as readable MCP tool errors", async () => {
    const call = vi.fn().mockResolvedValueOnce({ maxTimeMs: 45000 }).mockRejectedValueOnce(new Error("Disk full"));
    const response = await handler(call)({ params: { name: "export_video", arguments: { path: "/tmp/scene.mp4" } } });
    expect(response).toEqual({ isError: true, content: [{ type: "text", text: "Disk full" }] });
  });
});
