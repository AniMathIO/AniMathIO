import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const loader = require("../../../renderer/build/ffmpeg-static-worker-loader.js") as (
  source: string
) => string;

describe("ffmpeg-static-worker-loader", () => {
  it("strips the dynamic classWorkerURL branch from the real @ffmpeg/ffmpeg classes.js", () => {
    const source = readFileSync(
      require.resolve("@ffmpeg/ffmpeg").replace(/index\.js$|empty\.mjs$/, "classes.js"),
      "utf8"
    );
    const out = loader(source);
    expect(out).not.toContain("new URL(classWorkerURL");
    expect(out).toContain('new URL("./worker.js", import.meta.url)');
  });

  it("throws if the upstream source no longer has the expected shape", () => {
    expect(() => loader("export class FFmpeg {}")).toThrow(/package changed/);
  });
});
