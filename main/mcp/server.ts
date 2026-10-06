import { createServer, IncomingMessage, Server as HttpServer, ServerResponse } from "http";
import { timingSafeEqual } from "crypto";
import { Server as McpServer } from "@modelcontextprotocol/sdk/server/index.js";
import { StreamableHTTPServerTransport } from "@modelcontextprotocol/sdk/server/streamableHttp.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";
import { MCP_TOOLS } from "./tool-schemas";
import { RendererBridge } from "./bridge";

export const MCP_PATH = "/mcp";
const MAX_BODY_BYTES = 1_000_000;

function tokensMatch(provided: string, expected: string): boolean {
  const a = Buffer.from(provided);
  const b = Buffer.from(expected);
  // timingSafeEqual throws on length mismatch, which would itself leak length.
  if (a.length !== b.length) return false;
  return timingSafeEqual(a, b);
}

function readBody(req: IncomingMessage): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let size = 0;
    req.on("data", (chunk: Buffer) => {
      size += chunk.length;
      if (size > MAX_BODY_BYTES) {
        reject(new Error("Request body too large"));
        req.destroy();
        return;
      }
      chunks.push(chunk);
    });
    req.on("end", () => {
      const raw = Buffer.concat(chunks).toString("utf8");
      if (!raw) return resolve(undefined);
      try {
        resolve(JSON.parse(raw));
      } catch {
        reject(new Error("Request body is not valid JSON"));
      }
    });
    req.on("error", reject);
  });
}

export class AniMathIOMcpServer {
  private http: HttpServer | null = null;
  private transport: StreamableHTTPServerTransport | null = null;

  constructor(private readonly bridge: RendererBridge) {}

  get running(): boolean {
    return this.http !== null;
  }

  async start(port: number, token: string): Promise<void> {
    if (this.http) return;

    const mcp = new McpServer(
      { name: "animathio", version: "1.7.1" },
      { capabilities: { tools: {} } }
    );

    mcp.setRequestHandler(ListToolsRequestSchema, async () => ({
      tools: MCP_TOOLS,
    }));

    mcp.setRequestHandler(CallToolRequestSchema, async (request) => {
      const { name, arguments: args } = request.params;
      try {
        const data = await this.bridge.call(name, (args ?? {}) as Record<string, unknown>);
        return {
          content: [{ type: "text" as const, text: JSON.stringify(data, null, 2) }],
        };
      } catch (error) {
        // Surfaced to the agent as a tool error rather than a protocol error, so
        // it can read the reason and retry rather than losing the connection.
        return {
          isError: true,
          content: [
            {
              type: "text" as const,
              text: error instanceof Error ? error.message : String(error),
            },
          ],
        };
      }
    });

    // Stateless: every request carries its own context, so there are no sessions
    // to resume and nothing to clean up if an agent disconnects abruptly.
    this.transport = new StreamableHTTPServerTransport({
      sessionIdGenerator: undefined,
      enableJsonResponse: true,
    });
    await mcp.connect(this.transport);

    this.http = createServer((req, res) => {
      void this.handle(req, res, token);
    });

    await new Promise<void>((resolve, reject) => {
      const server = this.http!;
      server.once("error", reject);
      // Loopback only. This must never be reachable from the network.
      server.listen(port, "127.0.0.1", () => {
        server.removeListener("error", reject);
        resolve();
      });
    });
  }

  async stop(): Promise<void> {
    this.bridge.abortAll("The AniMathIO MCP server was stopped.");
    const server = this.http;
    this.http = null;
    if (this.transport) {
      await this.transport.close().catch(() => {});
      this.transport = null;
    }
    if (server) {
      await new Promise<void>((resolve) => server.close(() => resolve()));
    }
  }

  private async handle(req: IncomingMessage, res: ServerResponse, token: string) {
    const url = new URL(req.url ?? "/", "http://127.0.0.1");
    if (url.pathname !== MCP_PATH) {
      res.writeHead(404).end();
      return;
    }

    const auth = req.headers.authorization ?? "";
    const provided = auth.startsWith("Bearer ") ? auth.slice(7) : "";
    if (!provided || !tokensMatch(provided, token)) {
      res.writeHead(401, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: "Missing or invalid bearer token." }));
      return;
    }

    try {
      const body = req.method === "POST" ? await readBody(req) : undefined;
      await this.transport!.handleRequest(req, res, body);
    } catch (error) {
      if (!res.headersSent) {
        res.writeHead(400, { "content-type": "application/json" });
        res.end(
          JSON.stringify({
            error: error instanceof Error ? error.message : "Bad request",
          })
        );
      }
    }
  }
}
