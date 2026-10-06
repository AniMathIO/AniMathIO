/**
 * Wire format for the main <-> renderer MCP bridge.
 *
 * All editor state lives in the renderer's MobX RootStore, but the MCP server
 * runs in the main process, so every tool call is a round trip. Electron gives
 * us fire-and-forget `webContents.send` in that direction, so requests carry an
 * id the renderer echoes back to pair the reply with its caller.
 */

export const MCP_COMMAND_CHANNEL = "mcp-command";
export const MCP_RESULT_CHANNEL = "mcp-result";

export type McpCommand = {
  id: string;
  tool: string;
  args: Record<string, unknown>;
};

export type McpCommandResult =
  | { id: string; ok: true; data: unknown }
  | { id: string; ok: false; error: string };
