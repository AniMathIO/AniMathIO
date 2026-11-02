/**
 * Pure estimate of the box a plain-text element needs so its content does not
 * wrap or clip. Konva wraps on the element's fixed `width`, so a too-narrow
 * default splits words mid-word (e.g. "Smo/ke"). The estimate is deliberately
 * slightly generous (average glyph ~0.6em, wider for bold) because it cannot
 * measure the real font.
 */
export const MIN_TEXT_BOX = 100;
const LINE_HEIGHT = 1.2;

export function estimateTextBox(
  text: string,
  fontSize: number,
  fontWeight = 400,
  maxWidth = Infinity
): { width: number; height: number } {
  const lines = text.split(/\r?\n/);
  const longest = Math.max(...lines.map((line) => line.length), 1);
  const em = fontWeight >= 600 ? 0.65 : 0.6;
  const width = Math.max(MIN_TEXT_BOX, Math.ceil(longest * fontSize * em));
  const height = Math.max(MIN_TEXT_BOX, Math.ceil(lines.length * fontSize * LINE_HEIGHT));
  return { width: Math.min(width, Math.max(maxWidth, MIN_TEXT_BOX)), height };
}
