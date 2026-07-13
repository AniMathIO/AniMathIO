import { describe, it, expect, vi, beforeEach } from "vitest";

import {
  getCoverCrop,
  getFilterFromEffectType,
  KonvaAnimProxy,
  KonvaUtils,
  makeImageSceneFunc,
} from "../../../renderer/utils/konva-utils";

// Minimal Konva.Node mock
function makeNode(attrs: Record<string, number | boolean> = {}) {
  const store: Record<string, any> = { opacity: 1, scaleX: 1, scaleY: 1, x: 0, y: 0, visible: true, ...attrs };
  const node: any = {};
  for (const k of ["x", "y", "opacity", "scaleX", "scaleY"]) {
    node[k] = (v?: number) => (v !== undefined ? (store[k] = v) : store[k]);
  }
  node.visible = (v?: boolean) => (v !== undefined ? (store.visible = v) : store.visible);
  return node;
}

function makeLayer() {
  return { batchDraw: vi.fn() };
}

// Minimal Konva.Context/Shape mocks for makeImageSceneFunc
function makeSceneFuncArgs(width: number, height: number) {
  const ctx2d = {
    save: vi.fn(),
    restore: vi.fn(),
    drawImage: vi.fn(),
    strokeRect: vi.fn(),
    setLineDash: vi.fn(),
    filter: "none",
    strokeStyle: "",
    lineWidth: 0,
  };
  const context = {
    _context: ctx2d,
    beginPath: vi.fn(),
    rect: vi.fn(),
    closePath: vi.fn(),
  };
  const shape = {
    width: () => width,
    height: () => height,
  };
  return { ctx2d, context, shape };
}

describe("getCoverCrop", () => {
  it("fills width when target is wider than image", () => {
    // image 100x100, target 200x100 → needs to scale x
    const result = getCoverCrop(100, 100, 200, 100);
    expect(result.cropWidth).toBeLessThanOrEqual(100);
    expect(result.cropHeight).toBeLessThanOrEqual(100);
    expect(result.cropX).toBeGreaterThanOrEqual(0);
    expect(result.cropY).toBeGreaterThanOrEqual(0);
  });

  it("fills height when target is taller than image", () => {
    const result = getCoverCrop(100, 100, 100, 200);
    expect(result.cropWidth).toBeGreaterThan(0);
    expect(result.cropHeight).toBeGreaterThan(0);
  });

  it("handles identical ratios", () => {
    const result = getCoverCrop(200, 100, 400, 200);
    expect(result.cropX).toBe(0);
    expect(result.cropY).toBe(0);
    expect(result.cropWidth).toBe(200);
    expect(result.cropHeight).toBe(100);
  });

  it("always returns positive values", () => {
    const result = getCoverCrop(1, 1, 1920, 1080);
    expect(result.cropWidth).toBeGreaterThan(0);
    expect(result.cropHeight).toBeGreaterThan(0);
  });
});

describe("getFilterFromEffectType", () => {
  it("returns grayscale for blackAndWhite", () => {
    expect(getFilterFromEffectType("blackAndWhite")).toBe("grayscale(100%)");
  });
  it("returns sepia for sepia", () => {
    expect(getFilterFromEffectType("sepia")).toBe("sepia(100%)");
  });
  it("returns invert for invert", () => {
    expect(getFilterFromEffectType("invert")).toBe("invert(100%)");
  });
  it("returns none for none", () => {
    expect(getFilterFromEffectType("none")).toBe("none");
  });
});

describe("KonvaAnimProxy", () => {
  it("forwards left/top/opacity/scaleX/scaleY to Konva node and calls batchDraw", () => {
    const node = makeNode();
    const layer = makeLayer();
    const proxy = new KonvaAnimProxy(node as any, layer as any);

    proxy.left = 50;
    expect(node.x()).toBe(50);
    expect(layer.batchDraw).toHaveBeenCalled();

    proxy.top = 75;
    expect(node.y()).toBe(75);

    proxy.opacity = 0.5;
    expect(node.opacity()).toBe(0.5);

    proxy.scaleX = 2;
    expect(node.scaleX()).toBe(2);

    proxy.scaleY = 1.5;
    expect(node.scaleY()).toBe(1.5);
  });

  it("gets current values from node", () => {
    const node = makeNode({ x: 10, y: 20, opacity: 0.8 } as any);
    const layer = makeLayer();
    const proxy = new KonvaAnimProxy(node as any, layer as any);
    expect(proxy.left).toBe(10);
    expect(proxy.top).toBe(20);
    expect(proxy.opacity).toBe(0.8);
  });
});

describe("makeImageSceneFunc", () => {
  it("draws a healthy, fully-loaded image", () => {
    const { ctx2d, context, shape } = makeSceneFuncArgs(100, 50);
    const img = { complete: true, naturalWidth: 200, naturalHeight: 100 } as unknown as HTMLImageElement;

    const sceneFunc = makeImageSceneFunc(() => img, "none");
    sceneFunc(context as any, shape as any);

    expect(ctx2d.drawImage).toHaveBeenCalledTimes(1);
    expect(ctx2d.strokeRect).not.toHaveBeenCalled();
  });

  it("draws a placeholder instead of crashing for a broken image (naturalWidth 0)", () => {
    const { ctx2d, context, shape } = makeSceneFuncArgs(100, 50);
    // complete:true + naturalWidth:0 is exactly the "broken" state a failed <img> load leaves behind.
    const img = { complete: true, naturalWidth: 0, naturalHeight: 0, width: 20, height: 20 } as unknown as HTMLImageElement;

    const sceneFunc = makeImageSceneFunc(() => img, "none");
    sceneFunc(context as any, shape as any);

    expect(ctx2d.drawImage).not.toHaveBeenCalled();
    expect(ctx2d.strokeRect).toHaveBeenCalledTimes(1);
  });

  it("draws a placeholder when no element is available", () => {
    const { ctx2d, context, shape } = makeSceneFuncArgs(100, 50);

    const sceneFunc = makeImageSceneFunc(() => null, "none");
    sceneFunc(context as any, shape as any);

    expect(ctx2d.drawImage).not.toHaveBeenCalled();
    expect(ctx2d.strokeRect).toHaveBeenCalledTimes(1);
  });

  it("falls back to the placeholder if drawImage throws (InvalidStateError-style failure)", () => {
    const { ctx2d, context, shape } = makeSceneFuncArgs(100, 50);
    const img = { complete: true, naturalWidth: 200, naturalHeight: 100 } as unknown as HTMLImageElement;
    ctx2d.drawImage.mockImplementation(() => {
      throw new DOMException("broken state", "InvalidStateError");
    });
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});

    const sceneFunc = makeImageSceneFunc(() => img, "none");
    expect(() => sceneFunc(context as any, shape as any)).not.toThrow();

    expect(ctx2d.strokeRect).toHaveBeenCalledTimes(1);
    warnSpy.mockRestore();
  });

  it("treats a video below HAVE_CURRENT_DATA readyState as not drawable", () => {
    const { ctx2d, context, shape } = makeSceneFuncArgs(100, 50);
    const video = { readyState: 1, videoWidth: 640, videoHeight: 480 } as unknown as HTMLVideoElement;

    const sceneFunc = makeImageSceneFunc(() => video, "none");
    sceneFunc(context as any, shape as any);

    expect(ctx2d.drawImage).not.toHaveBeenCalled();
    expect(ctx2d.strokeRect).toHaveBeenCalledTimes(1);
  });

  it("draws a video once it has decoded a frame (readyState >= HAVE_CURRENT_DATA)", () => {
    const { ctx2d, context, shape } = makeSceneFuncArgs(100, 50);
    const video = { readyState: 2, videoWidth: 640, videoHeight: 480 } as unknown as HTMLVideoElement;

    const sceneFunc = makeImageSceneFunc(() => video, "none");
    sceneFunc(context as any, shape as any);

    expect(ctx2d.drawImage).toHaveBeenCalledTimes(1);
    expect(ctx2d.strokeRect).not.toHaveBeenCalled();
  });
});

describe("KonvaUtils.getClipRegion", () => {
  it("returns correct clip region with padding", () => {
    const element = {
      placement: { x: 100, y: 200, width: 300, height: 400, scaleX: 1, scaleY: 1, rotation: 0 },
    } as any;
    const region = KonvaUtils.getClipRegion(element, 10);
    expect(region.x).toBe(90);
    expect(region.y).toBe(190);
    expect(region.width).toBe(320);
    expect(region.height).toBe(420);
  });

  it("handles zero extra offset", () => {
    const element = {
      placement: { x: 50, y: 50, width: 200, height: 200, scaleX: 1, scaleY: 1, rotation: 0 },
    } as any;
    const region = KonvaUtils.getClipRegion(element, 0);
    expect(region.x).toBe(50);
    expect(region.y).toBe(50);
    expect(region.width).toBe(200);
    expect(region.height).toBe(200);
  });

  it("scales extra offset by scaleX/scaleY", () => {
    const element = {
      placement: { x: 0, y: 0, width: 100, height: 100, scaleX: 2, scaleY: 2, rotation: 0 },
    } as any;
    const region = KonvaUtils.getClipRegion(element, 10);
    // extraOffsetX = 10 / 2 = 5
    expect(region.x).toBe(-5);
    expect(region.width).toBe(110);
  });
});
