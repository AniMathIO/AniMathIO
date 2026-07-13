import { describe, it, expect } from "vitest";
import { classifyDroppedFile, computeDropCanvasPosition } from "../../../renderer/utils/dragDropFiles";

describe("classifyDroppedFile", () => {
  it("classifies a video file", () => {
    const file = new File(["data"], "clip.mp4", { type: "video/mp4" });
    expect(classifyDroppedFile(file)).toBe("video");
  });

  it("classifies an image file", () => {
    const file = new File(["data"], "photo.png", { type: "image/png" });
    expect(classifyDroppedFile(file)).toBe("image");
  });

  it("classifies an audio file", () => {
    const file = new File(["data"], "track.mp3", { type: "audio/mpeg" });
    expect(classifyDroppedFile(file)).toBe("audio");
  });

  it("returns null for an unsupported type", () => {
    const file = new File(["data"], "doc.pdf", { type: "application/pdf" });
    expect(classifyDroppedFile(file)).toBeNull();
  });

  it("returns null for an empty/unknown type", () => {
    const file = new File(["data"], "mystery", { type: "" });
    expect(classifyDroppedFile(file)).toBeNull();
  });
});

describe("computeDropCanvasPosition", () => {
  const canvasRect = { left: 100, top: 50, right: 500, bottom: 350, width: 400, height: 300 } as DOMRect;

  it("converts a point inside the canvas rect to stage coordinates", () => {
    // drop at the rect's center -> stage center
    const result = computeDropCanvasPosition(300, 200, canvasRect, 1920, 1080);
    expect(result.x).toBeCloseTo(960);
    expect(result.y).toBeCloseTo(540);
  });

  it("converts a point near the rect's corner correctly", () => {
    const result = computeDropCanvasPosition(100, 50, canvasRect, 1920, 1080);
    expect(result.x).toBeCloseTo(0);
    expect(result.y).toBeCloseTo(0);
  });

  it("falls back to canvas center when the point is outside the rect", () => {
    const result = computeDropCanvasPosition(50, 50, canvasRect, 1920, 1080);
    expect(result).toEqual({ x: 960, y: 540 });
  });

  it("falls back to canvas center when the rect is null (canvas not mounted)", () => {
    const result = computeDropCanvasPosition(300, 200, null, 1920, 1080);
    expect(result).toEqual({ x: 960, y: 540 });
  });
});
