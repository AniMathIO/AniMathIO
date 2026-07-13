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
 * Creates a custom Konva scene function that draws an image/video element
 * with cover-crop cropping and optional CSS-filter effects.
 */
export function makeImageSceneFunc(
  getElement: () => HTMLImageElement | HTMLVideoElement | null,
  effectType: EffectType
) {
  return (context: Konva.Context, shape: Konva.Shape) => {
    const element = getElement();
    if (!element) return;

    const ctx = (context as any)._context as CanvasRenderingContext2D;
    const width = shape.width();
    const height = shape.height();

    const naturalW =
      "naturalWidth" in element
        ? element.naturalWidth || (element as HTMLImageElement).width
        : (element as HTMLVideoElement).videoWidth || element.width;
    const naturalH =
      "naturalHeight" in element
        ? element.naturalHeight || (element as HTMLImageElement).height
        : (element as HTMLVideoElement).videoHeight || element.height;

    if (!naturalW || !naturalH || !width || !height) return;

    const { cropX, cropY, cropWidth, cropHeight } = getCoverCrop(
      naturalW,
      naturalH,
      width,
      height
    );

    ctx.save();
    const filter = getFilterFromEffectType(effectType);
    if (filter !== "none") {
      ctx.filter = filter;
    }
    ctx.drawImage(element, cropX, cropY, cropWidth, cropHeight, 0, 0, width, height);
    ctx.filter = "none";
    ctx.restore();

    // Draw the hit region so Konva can detect clicks/drags
    context.beginPath();
    context.rect(0, 0, width, height);
    context.closePath();
  };
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
