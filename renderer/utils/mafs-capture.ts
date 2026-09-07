import { getContrastColor } from "./color";

/**
 * Mafs's axis/grid colors are CSS custom properties declared only in the
 * `.MafsView { ... }` stylesheet rule. html-to-image's DOM clone never
 * embeds that rule (it lives inside a Tailwind v4 `@layer` block its
 * stylesheet walker doesn't recurse into), so every `var(--mafs-*)`
 * reference goes unresolved in the capture unless set as an inline style,
 * which does survive `cloneNode`. Mirrors core.css's defaults, except
 * `--mafs-fg`/`--mafs-origin-color`, which track the canvas's own
 * (independently configurable) background contrast.
 */
export function applyMafsCaptureStyles(mafsView: HTMLElement | null | undefined, backgroundColor: string): void {
  if (!mafsView) return;
  const fg = getContrastColor(backgroundColor);
  mafsView.style.setProperty("--mafs-fg", fg);
  mafsView.style.setProperty("--mafs-bg", "rgba(0, 0, 0, 0)");
  mafsView.style.setProperty("--mafs-origin-color", fg);
  mafsView.style.setProperty("--mafs-line-color", "#555");
  mafsView.style.setProperty("--mafs-line-stroke-dash-style", "4, 3");
  mafsView.style.setProperty("--mafs-axis-stroke-width", "1px");
  mafsView.style.setProperty("--grid-line-subdivision-color", "#222");
}
