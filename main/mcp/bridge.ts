import { randomUUID } from "crypto";
import { BrowserWindow, ipcMain } from "electron";
import {
  MCP_COMMAND_CHANNEL,
  MCP_RESULT_CHANNEL,
  McpCommandResult,
} from "./protocol";

const DEFAULT_TIMEOUT_MS = 30_000;

type PendingCall = {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: NodeJS.Timeout;
};

/**
 * Request/response over Electron's one-way main -> renderer channel.
 *
 * Nothing times out on its own here: a renderer that never replies (crashed,
 * reloading, or mid-navigation) would otherwise leave an MCP call hanging
 * forever, so every call carries its own deadline.
 */
export class RendererBridge {
  private pending = new Map<string, PendingCall>();
  private listening = false;

  constructor(private readonly getWindow: () => BrowserWindow | null) {}

  start() {
    if (this.listening) return;
    ipcMain.on(MCP_RESULT_CHANNEL, (_event, result: McpCommandResult) => {
      this.settle(result);
    });
    this.listening = true;
  }

  call(
    tool: string,
    args: Record<string, unknown>,
    timeoutMs: number = DEFAULT_TIMEOUT_MS
  ): Promise<unknown> {
    const window = this.getWindow();
    if (!window || window.isDestroyed()) {
      return Promise.reject(
        new Error("AniMathIO is not open. Start the app and open a project first.")
      );
    }

    const id = randomUUID();
    return new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(
          new Error(
            `The editor did not respond to "${tool}" within ${timeoutMs}ms. It may be busy or reloading.`
          )
        );
      }, timeoutMs);

      this.pending.set(id, { resolve, reject, timer });
      window.webContents.send(MCP_COMMAND_CHANNEL, { id, tool, args });
    });
  }

  /** Fail every in-flight call, e.g. when the server is being shut down. */
  abortAll(reason: string) {
    for (const [id, call] of this.pending) {
      clearTimeout(call.timer);
      this.pending.delete(id);
      call.reject(new Error(reason));
    }
  }

  private settle(result: McpCommandResult) {
    if (!result || typeof result.id !== "string") return;
    const call = this.pending.get(result.id);
    // A late reply after a timeout has no caller left to resolve; drop it.
    if (!call) return;

    clearTimeout(call.timer);
    this.pending.delete(result.id);

    if (result.ok) {
      call.resolve(result.data);
    } else {
      call.reject(new Error(result.error));
    }
  }
}
