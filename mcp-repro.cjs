const http = require("http");
const { Server } = require("@modelcontextprotocol/sdk/server/index.js");
const { StreamableHTTPServerTransport } = require("@modelcontextprotocol/sdk/server/streamableHttp.js");
const { ListToolsRequestSchema } = require("@modelcontextprotocol/sdk/types.js");

(async () => {
  const mcp = new Server({ name: "t", version: "1" }, { capabilities: { tools: {} } });
  mcp.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: [] }));
  const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined, enableJsonResponse: true });
  await mcp.connect(transport);

  const srv = http.createServer(async (req, res) => {
    let raw = "";
    for await (const c of req) raw += c;
    try {
      await transport.handleRequest(req, res, raw ? JSON.parse(raw) : undefined);
    } catch (e) {
      console.error("HANDLE ERROR:", e);
      if (!res.headersSent) res.writeHead(500).end(String(e));
    }
  });
  srv.listen(4599, "127.0.0.1", () => console.log("repro listening 4599"));
})();
