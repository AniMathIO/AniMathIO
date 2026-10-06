#!/usr/bin/env node
/**
 * Smoke-test client for AniMathIO's MCP server.
 *
 *   node scripts/mcp-smoke.mjs --token <token> [--port 4517]
 *   node scripts/mcp-smoke.mjs --token <token> --tool get_project_state
 *   node scripts/mcp-smoke.mjs --token <token> --tool add_text --args '{"text":"hi"}'
 */
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";

function parseFlags(argv) {
  const flags = {};
  for (let i = 0; i < argv.length; i++) {
    if (!argv[i].startsWith("--")) continue;
    const key = argv[i].slice(2);
    const next = argv[i + 1];
    flags[key] = next && !next.startsWith("--") ? (i++, next) : true;
  }
  return flags;
}

const flags = parseFlags(process.argv.slice(2));

if (flags.help) {
  console.log(`Usage: node scripts/mcp-smoke.mjs [options]

  --port <n>      default 4517, or ANIMATHIO_MCP_PORT
  --token <s>     required, or ANIMATHIO_MCP_TOKEN
  --tool <name>   call this tool instead of just listing
  --args '<json>' arguments for --tool (default {})`);
  process.exit(0);
}

const port = flags.port ?? process.env.ANIMATHIO_MCP_PORT ?? "4517";
const token = flags.token ?? process.env.ANIMATHIO_MCP_TOKEN;

if (!token) {
  console.error("No token. Pass --token or set ANIMATHIO_MCP_TOKEN (see Settings in the app).");
  process.exit(2);
}

const client = new Client({ name: "animathio-smoke", version: "1.0.0" }, { capabilities: {} });
const transport = new StreamableHTTPClientTransport(new URL(`http://127.0.0.1:${port}/mcp`), {
  requestInit: { headers: { Authorization: `Bearer ${token}` } },
});

function reportError(error) {
  console.error(`  ${error?.message ?? error}`);
  if (error?.code !== undefined) console.error(`  Code: ${error.code}`);
  if (error?.cause) {
    console.error(`  Cause: ${error.cause.code ?? ""} ${error.cause.message ?? error.cause}`);
  }
}

try {
  await client.connect(transport);
} catch (error) {
  console.error(`Could not connect to http://127.0.0.1:${port}/mcp`);
  reportError(error);
  console.error("Check that AniMathIO is running, the MCP server is enabled in Settings,");
  console.error("and that the port and token match what Settings shows.");
  await client.close().catch(() => {});
  process.exit(1);
}

try {
  if (flags.tool) {
    let args = {};
    if (typeof flags.args === "string") {
      try {
        args = JSON.parse(flags.args);
      } catch {
        console.error(`--args is not valid JSON: ${flags.args}`);
        process.exit(2);
      }
    }
    const result = await client.callTool({ name: flags.tool, arguments: args });
    if (result.isError) console.error("Tool reported an error:");
    for (const part of result.content ?? []) {
      console.log(part.type === "text" ? part.text : JSON.stringify(part));
    }
    process.exitCode = result.isError ? 1 : 0;
  } else {
    const { tools } = await client.listTools();
    console.log(`${tools.length} tools available:\n`);
    for (const tool of tools) {
      console.log(`  ${tool.name}`);
      console.log(`    ${(tool.description ?? "").split(". ")[0]}.`);
    }
  }
} catch (error) {
  console.error(`Request failed: ${error?.message ?? error}`);
  if (error?.cause || error?.code !== undefined) reportError(error);
  process.exitCode = 1;
} finally {
  await client.close().catch(() => {});
}
