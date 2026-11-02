import { describe, expect, it } from "vitest";
import { estimateTextBox, MIN_TEXT_BOX } from "@/utils/text-size";

describe("estimateTextBox", () => {
  it("is wide enough for the text at the given font size", () => {
    expect(estimateTextBox("Smoke test", 32).width).toBeGreaterThanOrEqual(10 * 32 * 0.55);
  });
  it("never goes below the minimum box", () => {
    expect(estimateTextBox("a", 10)).toEqual({ width: MIN_TEXT_BOX, height: MIN_TEXT_BOX });
  });
  it("uses the longest line and grows height per line", () => {
    const box = estimateTextBox("ab\nabcdefghij\nabc", 40);
    expect(box.width).toBe(Math.ceil(10 * 40 * 0.6));
    expect(box.height).toBe(Math.ceil(3 * 40 * 1.2));
  });
  it("is wider for bold text", () => {
    expect(estimateTextBox("Hello world", 30, 700).width).toBeGreaterThan(estimateTextBox("Hello world", 30, 400).width);
  });
  it("caps at the canvas width", () => {
    expect(estimateTextBox("x".repeat(200), 32, 400, 800).width).toBe(800);
  });
});
