import katex from "katex";
import * as htmlToImage from "html-to-image";

/**
 * Renders a LaTeX string to a rasterized PNG image via KaTeX + html-to-image,
 * since Konva's canvas Text node has no LaTeX/MathML support and can't
 * typeset math directly.
 */
export async function renderLatexToImage(
  tex: string,
  options?: { color?: string; displayMode?: boolean }
): Promise<{ dataUrl: string; width: number; height: number }> {
  const html = katex.renderToString(tex, {
    throwOnError: true,
    displayMode: options?.displayMode ?? true,
  });

  const container = document.createElement("div");
  // html-to-image screenshots real compositor output, so the container must
  // actually be painted: pushing it far off-screen (e.g. left: -99999px) is
  // outside the browser's paint bounds and rasterizes as fully transparent.
  // Keep it within the viewport, hidden behind everything and non-interactive
  // instead.
  container.style.position = "fixed";
  container.style.top = "0";
  container.style.left = "0";
  container.style.zIndex = "-1";
  container.style.pointerEvents = "none";
  container.style.color = options?.color ?? "#FFFFFF";
  container.style.fontSize = "40px";
  container.style.background = "transparent";
  container.innerHTML = html;
  document.body.appendChild(container);

  try {
    await document.fonts?.ready;
    // Give the browser a paint cycle before capturing.
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    const dataUrl = await htmlToImage.toPng(container, { pixelRatio: 2 });
    const rect = container.getBoundingClientRect();

    return {
      dataUrl,
      width: rect.width || 100,
      height: rect.height || 50,
    };
  } finally {
    document.body.removeChild(container);
  }
}
