import Konva from "konva";
import { EditorElement, EffectType } from "@/types";

/**
 * Proxy class that bridges anime.js property animation with Konva node setters.
 * anime.js sets plain object properties; this proxy translates each set into
 * the corresponding Konva node setter call and triggers a layer redraw.
 */
export class KonvaAnimProxy {
  constructor(
    private node: Konva.Node,
    private layer: Konva.Layer
  ) {}

  get left() { return this.node.x(); }
  set left(v: number) { this.node.x(v); this.layer.batchDraw(); }

  get top() { return this.node.y(); }
  set top(v: number) { this.node.y(v); this.layer.batchDraw(); }

  get opacity() { return this.node.opacity(); }
  set opacity(v: number) { this.node.opacity(v); this.layer.batchDraw(); }

  get scaleX() { return this.node.scaleX(); }
  set scaleX(v: number) { this.node.scaleX(v); this.layer.batchDraw(); }

  get scaleY() { return this.node.scaleY(); }
  set scaleY(v: number) { this.node.scaleY(v); this.layer.batchDraw(); }
}

/**
 * Calculates the cover-crop parameters (source region) so that an image/video
 * fills the target rect without letterboxing (equivalent to CSS object-fit: cover).
 */
export function getCoverCrop(
  naturalW: number,
  naturalH: number,
  targetW: number,
  targetH: number
): { cropX: number; cropY: number; cropWidth: number; cropHeight: number } {
  const targetRatio = targetW / targetH;
  const imageRatio = naturalW / naturalH;

  let cropWidth: number;
  let cropHeight: number;
  let cropX: number;
  let cropY: number;

  if (targetRatio >= imageRatio) {
    cropWidth = naturalW;
    cropHeight = naturalW / targetRatio;
    cropX = 0;
    cropY = (naturalH - cropHeight) / 2;
  } else {
    cropHeight = naturalH;
    cropWidth = naturalH * targetRatio;
    cropX = (naturalW - cropWidth) / 2;
    cropY = 0;
  }

  return {
    cropX: Math.max(0, cropX),
    cropY: Math.max(0, cropY),
    cropWidth: Math.max(1, cropWidth),
    cropHeight: Math.max(1, cropHeight),
  };
}

/**
 * Returns the CSS filter string for a given effect type.
 * Used in custom scene functions where the canvas 2D context filter is set directly.
 */
export function getFilterFromEffectType(effectType: EffectType): string {
  switch (effectType) {
    case "blackAndWhite":
      return "grayscale(100%)";
    case "sepia":
      return "sepia(100%)";
    case "invert":
      return "invert(100%)";
    case "saturate":
      return "saturate(200%)";
    default:
      return "none";
  }
}

/**
 * Whether an <img>/<video> element has actually finished loading real pixel
 * data and is safe to pass to ctx.drawImage(). A failed/unresolvable src
 * still leaves the element in the DOM with `complete: true` but no decoded
 * bitmap ("broken" state) — drawImage() throws InvalidStateError for that,
 * so this must be checked instead of just truthiness of width/height (which
 * can come from HTML width/height attributes set for unrelated layout
 * reasons, not actual image data).
 */
function isElementDrawable(element: HTMLImageElement | HTMLVideoElement): boolean {
  if ("naturalWidth" in element) {
    return element.complete && element.naturalWidth > 0 && element.naturalHeight > 0;
  }
  // HTMLVideoElement: HAVE_CURRENT_DATA (2) or above means a frame is decoded.
  return element.readyState >= 2 && element.videoWidth > 0 && element.videoHeight > 0;
}

/** Draws a dashed placeholder rect for a missing/broken image or video. */
function drawBrokenPlaceholder(ctx: CanvasRenderingContext2D, width: number, height: number) {
  ctx.save();
  ctx.strokeStyle = "rgba(255, 255, 255, 0.4)";
  ctx.setLineDash([6, 4]);
  ctx.lineWidth = 2;
  ctx.strokeRect(1, 1, Math.max(0, width - 2), Math.max(0, height - 2));
  ctx.restore();
}

/**
 * Creates a custom Konva scene function that draws an image/video element
 * with cover-crop cropping and optional CSS-filter effects.
 */
export function makeImageSceneFunc(
  getElement: () => HTMLImageElement | HTMLVideoElement | null,
  effectType: EffectType
) {
  return (context: Konva.Context, shape: Konva.Shape) => {
    const element = getElement();
    const width = shape.width();
    const height = shape.height();
    const ctx = (context as any)._context as CanvasRenderingContext2D;

    if (!width || !height) return;

    if (!element || !isElementDrawable(element)) {
      drawBrokenPlaceholder(ctx, width, height);
      context.beginPath();
      context.rect(0, 0, width, height);
      context.closePath();
      return;
    }

    const naturalW = "naturalWidth" in element ? element.naturalWidth : element.videoWidth;
    const naturalH = "naturalHeight" in element ? element.naturalHeight : element.videoHeight;

    const { cropX, cropY, cropWidth, cropHeight } = getCoverCrop(
      naturalW,
      naturalH,
      width,
      height
    );

    try {
      ctx.save();
      const filter = getFilterFromEffectType(effectType);
      if (filter !== "none") {
        ctx.filter = filter;
      }
      ctx.drawImage(element, cropX, cropY, cropWidth, cropHeight, 0, 0, width, height);
      ctx.filter = "none";
      ctx.restore();
    } catch (error) {
      // Defense-in-depth: a single broken element must never abort Konva's
      // synchronous layer draw pass, which would hide every element drawn
      // after it in the same layer.
      console.warn("Failed to draw image/video element:", error);
      drawBrokenPlaceholder(ctx, width, height);
    }

    // Draw the hit region so Konva can detect clicks/drags
    context.beginPath();
    context.rect(0, 0, width, height);
    context.closePath();
  };
}

// ============================================================
// Snapping guidelines
// ============================================================

export type SnapGuides = { vertical: number[]; horizontal: number[] };
export const NO_GUIDES: SnapGuides = { vertical: [], horizontal: [] };
const SNAP_THRESHOLD = 6;

/**
 * Candidate stop coordinates a dragged node can snap to: the canvas edges
 * and center, plus every other element's edges and center (elements must be
 * named "editorElement" in the Konva tree for stage.find to pick them up).
 */
export function getLineGuideStops(skipShape: Konva.Node, stage: Konva.Stage) {
  const vertical: number[] = [0, stage.width() / 2, stage.width()];
  const horizontal: number[] = [0, stage.height() / 2, stage.height()];

  stage.find(".editorElement").forEach((guideItem) => {
    if (guideItem === skipShape) return;
    const box = guideItem.getClientRect();
    vertical.push(box.x, box.x + box.width / 2, box.x + box.width);
    horizontal.push(box.y, box.y + box.height / 2, box.y + box.height);
  });

  return { vertical, horizontal };
}

/** The dragged node's own edge/center coordinates, paired with the offset
 * needed to reposition the node so that edge/center lands on a given stop. */
export function getObjectSnappingEdges(node: Konva.Node) {
  const box = node.getClientRect();
  return {
    vertical: [
      { guide: box.x, offset: 0 },
      { guide: box.x + box.width / 2, offset: box.width / 2 },
      { guide: box.x + box.width, offset: box.width },
    ],
    horizontal: [
      { guide: box.y, offset: 0 },
      { guide: box.y + box.height / 2, offset: box.height / 2 },
      { guide: box.y + box.height, offset: box.height },
    ],
  };
}

/**
 * Snaps a dragged node's absolute position to the nearest canvas/element edge
 * or center within SNAP_THRESHOLD px, and returns the guide-line positions to
 * render (empty when nothing is close enough to snap to).
 */
export function snapNodeToGuides(node: Konva.Node, stage: Konva.Stage): SnapGuides {
  const lineGuideStops = getLineGuideStops(node, stage);
  const itemBounds = getObjectSnappingEdges(node);

  const closestVertical = itemBounds.vertical
    .flatMap((item) =>
      lineGuideStops.vertical.map((guide) => ({ guide, offset: item.offset, diff: Math.abs(guide - item.guide) }))
    )
    .filter((candidate) => candidate.diff < SNAP_THRESHOLD)
    .sort((a, b) => a.diff - b.diff)[0];

  const closestHorizontal = itemBounds.horizontal
    .flatMap((item) =>
      lineGuideStops.horizontal.map((guide) => ({ guide, offset: item.offset, diff: Math.abs(guide - item.guide) }))
    )
    .filter((candidate) => candidate.diff < SNAP_THRESHOLD)
    .sort((a, b) => a.diff - b.diff)[0];

  if (!closestVertical && !closestHorizontal) return NO_GUIDES;

  const absPos = node.absolutePosition();
  if (closestVertical) absPos.x = closestVertical.guide - closestVertical.offset;
  if (closestHorizontal) absPos.y = closestHorizontal.guide - closestHorizontal.offset;
  node.absolutePosition(absPos);

  return {
    vertical: closestVertical ? [closestVertical.guide] : [],
    horizontal: closestHorizontal ? [closestHorizontal.guide] : [],
  };
}

/**
 * dragBoundFunc helper: clamps a node's proposed drag position so its
 * (axis-aligned) bounding box stays within [0, stageWidth] x [0, stageHeight].
 * getClientRect() is used instead of raw width/height so rotated nodes are
 * clamped by their actual on-screen footprint, not their unrotated box.
 * If the node itself is larger than the stage on an axis, that axis is left
 * unclamped rather than locking the node into a degenerate (min > max) range.
 */
export function clampNodeToStage(
  node: Konva.Node,
  pos: { x: number; y: number },
  stageWidth: number,
  stageHeight: number
): { x: number; y: number } {
  const rect = node.getClientRect();
  const currentPos = node.position();
  const offsetX = rect.x - currentPos.x;
  const offsetY = rect.y - currentPos.y;

  let x = pos.x;
  let y = pos.y;

  if (rect.width <= stageWidth) {
    x = Math.min(Math.max(pos.x, -offsetX), stageWidth - rect.width - offsetX);
  }
  if (rect.height <= stageHeight) {
    y = Math.min(Math.max(pos.y, -offsetY), stageHeight - rect.height - offsetY);
  }

  return { x, y };
}

/**
 * Returns a clip region object for use in slideIn/slideOut animations.
 * In Konva, clip is defined as { x, y, width, height } on a Group or Node.
 */
export class KonvaUtils {
  static getClipRegion(editorElement: EditorElement, extraOffset: number) {
    const { x, y, width, height, scaleX, scaleY } = editorElement.placement;
    const extraOffsetX = extraOffset / scaleX;
    const extraOffsetY = extraOffset / scaleY;
    return {
      x: x - extraOffsetX,
      y: y - extraOffsetY,
      width: width + extraOffsetX * 2,
      height: height + extraOffsetY * 2,
    };
  }
}
