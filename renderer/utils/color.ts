/**
 * Returns a contrasting foreground color ("#FFFFFF" or "#000000") for a given
 * background color, using the standard relative-luminance threshold. Used to
 * pick a readable default for elements (Manim text, Mafs graphs) that don't
 * specify their own color, instead of hardcoding white — which is invisible
 * against a light canvas background.
 */
export function getContrastColor(backgroundColor: string): "#FFFFFF" | "#000000" {
  const { r, g, b } = parseColorToRgb(backgroundColor);
  // Standard relative luminance (per WCAG / ITU-R BT.601).
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luminance > 0.5 ? "#000000" : "#FFFFFF";
}

function parseColorToRgb(color: string): { r: number; g: number; b: number } {
  const hex = color.trim().replace(/^#/, "");
  const expanded =
    hex.length === 3
      ? hex
          .split("")
          .map((c) => c + c)
          .join("")
      : hex;

  if (expanded.length !== 6 || !/^[0-9a-fA-F]{6}$/.test(expanded)) {
    // Unparseable input: assume a dark background (the app's own default).
    return { r: 17, g: 17, b: 17 };
  }

  return {
    r: parseInt(expanded.slice(0, 2), 16),
    g: parseInt(expanded.slice(2, 4), 16),
    b: parseInt(expanded.slice(4, 6), 16),
  };
}
