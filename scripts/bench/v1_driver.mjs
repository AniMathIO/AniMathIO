#!/usr/bin/env node
// MCP client for the packaged v1 (Electron) app, used only by scripts/bench-v1-v2.sh.
//
// v1 has no headless or CLI mode; its one scripted interface is the opt-in loopback MCP server
// (`main/mcp/*`). This driver talks to it with the MCP SDK that ships in v1's own node_modules,
// so nothing v1-related is vendored here.
//
//   node v1_driver.mjs wait   --v1 DIR --port N --token T [--want ELEMENTS] [--timeout MS]
//   node v1_driver.mjs call   --v1 DIR --port N --token T --script steps.json
//   node v1_driver.mjs export --v1 DIR --port N --token T --path OUT.mp4 [--timeout MS]
//   node v1_driver.mjs seek   --v1 DIR --port N --token T [--seconds 10] [--hz 60]
//
// `--v1 DIR` points at the v1 checkout: the MCP SDK is loaded from that tree's
// own node_modules (the packaged app ships it in app.asar, but the version is
// identical, so the source tree copy is used to keep this driver dependency-free).
//
// Every command prints one JSON object on stdout. Timestamps are `Date.now()` epoch
// milliseconds so the harness can subtract its own launch time from the same clock.
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const argv = process.argv.slice(2);
const cmd = argv.shift();
const flags = {};
for (let i = 0; i < argv.length; i++) {
  if (!argv[i].startsWith("--")) continue;
  flags[argv[i].slice(2)] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true;
}
const need = (name) => {
  if (flags[name] === undefined) {
    console.error(`missing --${name}`);
    process.exit(2);
  }
  return flags[name];
};

const v1 = need("v1");
const port = Number(flags.port ?? 4517);
const token = need("token");
const sdk = (rel) => import(pathToFileURL(`${v1}/node_modules/@modelcontextprotocol/sdk/dist/esm/${rel}`).href);
const { Client } = await sdk("client/index.js");
const { StreamableHTTPClientTransport } = await sdk("client/streamableHttp.js");

async function connect() {
  const client = new Client({ name: "animathio-bench", version: "1.0.0" }, { capabilities: {} });
  const transport = new StreamableHTTPClientTransport(new URL(`http://127.0.0.1:${port}/mcp`), {
    requestInit: { headers: { Authorization: `Bearer ${token}` } },
  });
  await client.connect(transport);
  return client;
}

async function tool(client, name, args = {}, timeout = 60_000) {
  const result = await client.callTool({ name, arguments: args }, undefined, { timeout });
  const text = result.content?.map((part) => part.text ?? "").join("") ?? "";
  if (result.isError) throw new Error(`${name}: ${text}`);
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function connectWithRetry(timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      const client = await connect();
      return { client, at: Date.now() };
    } catch (error) {
      if (Date.now() > deadline) throw new Error(`MCP server never came up: ${error.message}`);
      await sleep(25);
    }
  }
}

if (cmd === "wait") {
  const timeout = Number(flags.timeout ?? 120_000);
  const want = Number(flags.want ?? 0);
  const { client, at: mcpReady } = await connectWithRetry(timeout);
  const deadline = Date.now() + timeout;
  let state = await tool(client, "get_project_state");
  // `--want 0` (cold start) is satisfied by the first answer; otherwise the editor
  // must be active and show at least `want` elements.
  while (!(state.elements.length >= want && (want === 0 || state.isEditorActive))) {
    if (Date.now() > deadline) throw new Error(`project never loaded (state: ${JSON.stringify(state)})`);
    await sleep(25);
    state = await tool(client, "get_project_state");
  }
  const loaded = Date.now();
  console.log(JSON.stringify({ mcpReady, loaded, elements: state.elements.length, maxTimeMs: state.maxTimeMs, canvas: state.canvas }));
  await client.close();
} else if (cmd === "call") {
  const steps = JSON.parse(readFileSync(need("script"), "utf8"));
  const { client } = await connectWithRetry(60_000);
  const out = [];
  // `$N.field` or `$N.field.0` in a string argument refers to the Nth earlier result
  // (optionally indexing an array such as add_media's ids), so a script can reuse ids.
  for (const step of steps) {
    const args = JSON.parse(
      JSON.stringify(step.args ?? {}).replace(
        /"\$(\d+)\.([A-Za-z_]+)(?:\.(\d+))?"/g,
        (_m, n, key, idx) => {
          const value = out[Number(n)]?.[key];
          return JSON.stringify(idx === undefined ? value : value?.[Number(idx)]);
        },
      ),
    );
    out.push(await tool(client, step.tool, args, step.timeout ?? 120_000));
  }
  console.log(JSON.stringify(out));
  await client.close();
} else if (cmd === "export") {
  const { client } = await connectWithRetry(60_000);
  const state = await tool(client, "get_project_state");
  const timeout = Math.ceil(state.maxTimeMs * 3 + 300_000);
  const started = Date.now();
  await tool(client, "export_video", { path: need("path") }, Number(flags.timeout ?? timeout));
  const finished = Date.now();
  console.log(JSON.stringify({ started, finished, seconds: (finished - started) / 1000, maxTimeMs: state.maxTimeMs }));
  await client.close();
} else if (cmd === "seek") {
  // Scrub: ask the renderer to seek to a deterministic pseudo-random time at `hz` requests per
  // second. Each call round-trips main -> renderer -> main, so the latency below is the time the
  // editor needs to apply the seek (MobX update; React/Konva paint are scheduled after it).
  const { client } = await connectWithRetry(60_000);
  const state = await tool(client, "get_project_state");
  const seconds = Number(flags.seconds ?? 10);
  const hz = Number(flags.hz ?? 60);
  let seed = 12345;
  const next = () => ((seed = (seed * 1103515245 + 12345) >>> 0) / 2 ** 32);
  const latencies = [];
  const end = Date.now() + seconds * 1000;
  for (let n = 0; Date.now() < end; n++) {
    const due = Date.now() + 1000 / hz;
    const t0 = performance.now();
    await tool(client, "seek", { timeMs: Math.floor(next() * state.maxTimeMs) });
    latencies.push(performance.now() - t0);
    const wait = due - Date.now();
    if (wait > 0) await sleep(wait);
  }
  latencies.sort((a, b) => a - b);
  const pct = (p) => latencies[Math.min(latencies.length - 1, Math.floor(p * latencies.length))];
  console.log(JSON.stringify({ samples: latencies.length, p50_ms: pct(0.5), p95_ms: pct(0.95), max_ms: latencies.at(-1) }));
  await client.close();
} else {
  console.error("usage: v1_driver.mjs wait|call|export|seek ...");
  process.exit(2);
}
process.exit(0);
