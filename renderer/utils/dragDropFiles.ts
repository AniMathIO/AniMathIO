export type DroppedFileKind = "video" | "image" | "audio";

/** Classifies a dropped browser File by its MIME type prefix. */
export function classifyDroppedFile(file: File): DroppedFileKind | null {
  if (file.type.startsWith("video/")) return "video";
  if (file.type.startsWith("image/")) return "image";
  if (file.type.startsWith("audio/")) return "audio";
  return null;
}

/**
 * Converts a drop point (browser client coordinates) into Konva stage
 * coordinates when it lands inside the canvas, mirroring the CSS-scale-aware
 * math the panel->canvas drag-and-drop drop handler already uses. When the
 * point falls outside the canvas (dropped on a sidebar panel) or the canvas
 * isn't mounted yet, falls back to the canvas center instead of stacking
 * everything at (0, 0).
 */
export function computeDropCanvasPosition(
  clientX: number,
  clientY: number,
  canvasRect: DOMRect | null,
  stageWidth: number,
  stageHeight: number
): { x: number; y: number } {
  if (
    canvasRect &&
    clientX >= canvasRect.left &&
    clientX <= canvasRect.right &&
    clientY >= canvasRect.top &&
    clientY <= canvasRect.bottom &&
    canvasRect.width > 0 &&
    canvasRect.height > 0
  ) {
    return {
      x: ((clientX - canvasRect.left) / canvasRect.width) * stageWidth,
      y: ((clientY - canvasRect.top) / canvasRect.height) * stageHeight,
    };
  }

  return { x: stageWidth / 2, y: stageHeight / 2 };
}
