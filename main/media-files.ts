import { readFile, writeFile } from "fs/promises";
import path from "path";
import type { IpcMain } from "electron";

const MEDIA_TYPES: Record<string, ["image" | "video" | "audio", string]> = {
  ".png": ["image", "image/png"],
  ".jpg": ["image", "image/jpeg"],
  ".jpeg": ["image", "image/jpeg"],
  ".gif": ["image", "image/gif"],
  ".webp": ["image", "image/webp"],
  ".svg": ["image", "image/svg+xml"],
  ".bmp": ["image", "image/bmp"],
  ".mp4": ["video", "video/mp4"],
  ".webm": ["video", "video/webm"],
  ".mov": ["video", "video/quicktime"],
  ".m4v": ["video", "video/mp4"],
  ".mp3": ["audio", "audio/mpeg"],
  ".wav": ["audio", "audio/wav"],
  ".ogg": ["audio", "audio/ogg"],
  ".m4a": ["audio", "audio/mp4"],
  ".aac": ["audio", "audio/aac"],
  ".flac": ["audio", "audio/flac"],
};

function requireAbsolutePath(filePath: string) {
  if (typeof filePath !== "string" || !path.isAbsolute(filePath)) {
    throw new Error("An absolute filesystem path is required.");
  }
}

export async function readMediaFile(filePath: string) {
  try {
    requireAbsolutePath(filePath);
    const media = MEDIA_TYPES[path.extname(filePath).toLowerCase()];
    if (!media) throw new Error(`Unsupported media type: ${path.extname(filePath) || "no extension"}.`);
    const bytes = await readFile(filePath);
    return { success: true as const, type: media[0], dataUrl: `data:${media[1]};base64,${bytes.toString("base64")}` };
  } catch (error) {
    return { success: false as const, error: `Could not read media file "${filePath}": ${error instanceof Error ? error.message : String(error)}` };
  }
}

export async function writeVideoFile(filePath: string, bytes: number[]) {
  try {
    requireAbsolutePath(filePath);
    if (![".mp4", ".webm"].includes(path.extname(filePath).toLowerCase())) {
      throw new Error("Video destination must end in .mp4 or .webm.");
    }
    await writeFile(filePath, Buffer.from(bytes));
    return { success: true as const };
  } catch (error) {
    return { success: false as const, error: error instanceof Error ? error.message : String(error) };
  }
}

export function registerMediaFileHandlers(ipc: Pick<IpcMain, "handle">) {
  ipc.handle("read-media-file", (_event, filePath: string) => readMediaFile(filePath));
  ipc.handle("write-video-file", (_event, filePath: string, fileData: number[]) => writeVideoFile(filePath, fileData));
}
