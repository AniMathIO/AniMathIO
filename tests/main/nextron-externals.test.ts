import { describe, expect, it, vi } from "vitest";
import config from "../../nextron.config.js";

describe("Nextron MCP runtime externals", () => {
  it.each(["@modelcontextprotocol/sdk/server/index.js", "@modelcontextprotocol/sdk/server/streamableHttp.js", "@hono/node-server"])(
    "keeps %s external and retains existing externals", (request) => {
      const existing = ["electron-store"];
      const result = config.webpack({ externals: existing });
      expect(result.externals[0]).toBe("electron-store");
      const callback = vi.fn();
      result.externals[1]({ request }, callback);
      expect(callback).toHaveBeenCalledExactlyOnceWith(null, `commonjs ${request}`);
    }
  );
  it("allows unrelated modules to use the normal bundle resolution", () => {
    const result = config.webpack({});
    const callback = vi.fn();
    result.externals[0]({ request: "./mcp/tool-call" }, callback);
    expect(callback).toHaveBeenCalledExactlyOnceWith();
  });
});
