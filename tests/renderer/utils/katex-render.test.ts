import { describe, it, expect, vi, beforeEach } from "vitest";

const renderToStringMock = vi.fn((tex: string) => `<span class="katex-mock">${tex}</span>`);
const toPngMock = vi.fn(async () => "data:image/png;base64,mocked");

vi.mock("katex", () => ({
  default: { renderToString: (...args: unknown[]) => renderToStringMock(...(args as [string])) },
}));

vi.mock("html-to-image", () => ({
  toPng: (...args: unknown[]) => toPngMock(...(args as [HTMLElement])),
}));

import { renderLatexToImage } from "../../../renderer/utils/katex-render";

describe("renderLatexToImage", () => {
  beforeEach(() => {
    renderToStringMock.mockClear();
    toPngMock.mockClear();
    document.body.innerHTML = "";
  });

  it("renders KaTeX HTML into an off-screen container and rasterizes it", async () => {
    const result = await renderLatexToImage("E = mc^2");

    expect(renderToStringMock).toHaveBeenCalledWith(
      "E = mc^2",
      expect.objectContaining({ throwOnError: true, displayMode: true })
    );
    expect(toPngMock).toHaveBeenCalledTimes(1);
    expect(result.dataUrl).toBe("data:image/png;base64,mocked");
  });

  it("applies the requested color and displayMode to the container/options", async () => {
    await renderLatexToImage("x^2", { color: "#FFFF00", displayMode: false });

    expect(renderToStringMock).toHaveBeenCalledWith(
      "x^2",
      expect.objectContaining({ displayMode: false })
    );
    const container = toPngMock.mock.calls[0][0] as HTMLElement;
    expect(container.style.color).toBe("#FFFF00");
  });

  it("removes the off-screen container from the DOM after rendering", async () => {
    await renderLatexToImage("a + b");
    expect(document.body.children.length).toBe(0);
  });

  it("cleans up the container even if rasterization fails", async () => {
    toPngMock.mockRejectedValueOnce(new Error("rasterization failed"));

    await expect(renderLatexToImage("bad")).rejects.toThrow("rasterization failed");
    expect(document.body.children.length).toBe(0);
  });

  it("propagates KaTeX parse errors", async () => {
    renderToStringMock.mockImplementationOnce(() => {
      throw new Error("KaTeX parse error");
    });

    await expect(renderLatexToImage("\\invalidcommand")).rejects.toThrow("KaTeX parse error");
  });
});
