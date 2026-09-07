import { describe, it, expect } from "vitest";
import { getContrastColor } from "../../../renderer/utils/color";

describe("getContrastColor", () => {
  it("returns white for a dark background", () => {
    expect(getContrastColor("#111111")).toBe("#FFFFFF");
    expect(getContrastColor("#000000")).toBe("#FFFFFF");
  });

  it("returns black for a light background", () => {
    expect(getContrastColor("#FFFFFF")).toBe("#000000");
    expect(getContrastColor("#EEEEEE")).toBe("#000000");
  });

  it("handles 3-digit hex shorthand", () => {
    expect(getContrastColor("#000")).toBe("#FFFFFF");
    expect(getContrastColor("#fff")).toBe("#000000");
  });

  it("handles hex without a leading #", () => {
    expect(getContrastColor("111111")).toBe("#FFFFFF");
  });

  it("falls back to a dark-background assumption for unparseable input", () => {
    expect(getContrastColor("not-a-color")).toBe("#FFFFFF");
    expect(getContrastColor("")).toBe("#FFFFFF");
  });
});
