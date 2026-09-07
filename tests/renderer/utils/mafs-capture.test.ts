import { describe, it, expect } from "vitest";
import { applyMafsCaptureStyles } from "../../../renderer/utils/mafs-capture";

describe("applyMafsCaptureStyles", () => {
  it("sets all mafs custom properties as inline styles so html-to-image's clone carries them", () => {
    const el = document.createElement("div");
    applyMafsCaptureStyles(el, "#000000");

    expect(el.style.getPropertyValue("--mafs-fg")).toBe("#FFFFFF");
    expect(el.style.getPropertyValue("--mafs-origin-color")).toBe("#FFFFFF");
    expect(el.style.getPropertyValue("--mafs-bg")).toBe("rgba(0, 0, 0, 0)");
    expect(el.style.getPropertyValue("--mafs-line-color")).toBe("#555");
    expect(el.style.getPropertyValue("--mafs-line-stroke-dash-style")).toBe("4, 3");
    expect(el.style.getPropertyValue("--mafs-axis-stroke-width")).toBe("1px");
    expect(el.style.getPropertyValue("--grid-line-subdivision-color")).toBe("#222");
  });

  it("derives --mafs-fg/--mafs-origin-color from the canvas background's contrast color", () => {
    const el = document.createElement("div");
    applyMafsCaptureStyles(el, "#FFFFFF");

    expect(el.style.getPropertyValue("--mafs-fg")).toBe("#000000");
    expect(el.style.getPropertyValue("--mafs-origin-color")).toBe("#000000");
  });

  it("does nothing (no throw) when the .MafsView element isn't found", () => {
    expect(() => applyMafsCaptureStyles(null, "#000000")).not.toThrow();
    expect(() => applyMafsCaptureStyles(undefined, "#000000")).not.toThrow();
  });
});
