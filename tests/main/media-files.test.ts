import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtemp, readFile, rm, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { readMediaFile, writeVideoFile } from "../../main/media-files";

describe("media file IPC operations", () => {
  let dir: string;
  beforeEach(async () => { dir = await mkdtemp(path.join(tmpdir(), "animathio-media-")); });
  afterEach(async () => { await rm(dir, { recursive: true, force: true }); });

  it.each([["PNG", "image", "image/png"], ["mp4", "video", "video/mp4"], ["mp3", "audio", "audio/mpeg"]])(
    "reads %s bytes as the correct data URL and media kind", async (extension, type, mime) => {
      const file = path.join(dir, `media.${extension}`);
      const bytes = Buffer.from([0, 1, 127, 128, 255]);
      await writeFile(file, bytes);
      expect(await readMediaFile(file)).toEqual({ success: true, type, dataUrl: `data:${mime};base64,${bytes.toString("base64")}` });
    }
  );
  it("reports missing files with their path", async () => {
    const file = path.join(dir, "missing.png");
    expect(await readMediaFile(file)).toMatchObject({ success: false, error: expect.stringContaining(file) });
  });
  it("reports unsupported types and relative paths", async () => {
    expect(await readMediaFile(path.join(dir, "file.txt"))).toMatchObject({ success: false, error: expect.stringContaining("Unsupported media type") });
    expect(await readMediaFile("file.png")).toMatchObject({ success: false, error: expect.stringContaining("absolute") });
  });
  it.each(["mp4", "webm"])("writes %s bytes exactly at the requested path, replacing existing content", async (extension) => {
    const file = path.join(dir, `export.${extension}`);
    await writeFile(file, "old longer content");
    expect(await writeVideoFile(file, [0, 127, 128, 255])).toEqual({ success: true });
    expect(await readFile(file)).toEqual(Buffer.from([0, 127, 128, 255]));
  });
  it("returns disk errors and refuses unsupported output paths", async () => {
    expect(await writeVideoFile(path.join(dir, "missing", "render.webm"), [1])).toMatchObject({ success: false, error: expect.stringContaining("ENOENT") });
    expect(await writeVideoFile(path.join(dir, "render.txt"), [1])).toMatchObject({ success: false, error: expect.stringContaining(".mp4 or .webm") });
    expect(await writeVideoFile("render.webm", [1])).toMatchObject({ success: false, error: expect.stringContaining("absolute") });
  });
});

import { vi } from "vitest";
import { registerMediaFileHandlers } from "../../main/media-files";
it("registers the actual media IPC handlers and forwards paths and bytes", async () => {
  const handlers = new Map<string, (...args: any[]) => Promise<any>>();
  const handle = vi.fn((channel, handler) => { handlers.set(channel, handler); });
  registerMediaFileHandlers({ handle } as any);
  expect([...handlers.keys()]).toEqual(["read-media-file", "write-video-file"]);
  const dir = await mkdtemp(path.join(tmpdir(), "animathio-ipc-"));
  try {
    const file = path.join(dir, "movie.webm");
    await expect(handlers.get("write-video-file")!({}, file, [0, 255])).resolves.toEqual({ success: true });
    expect(await readFile(file)).toEqual(Buffer.from([0, 255]));
    await expect(handlers.get("read-media-file")!({}, file)).resolves.toEqual({ success: true, type: "video", dataUrl: "data:video/webm;base64,AP8=" });
  } finally { await rm(dir, { recursive: true, force: true }); }
});
