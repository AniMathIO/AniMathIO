import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { State } from "../../../renderer/states/state";

vi.mock("konva", () => ({
  default: {
    Stage: vi.fn(() => ({ width: vi.fn(), height: vi.fn(), container: vi.fn(() => ({ style: { backgroundColor: "" } })) })),
    Layer: vi.fn(() => ({ batchDraw: vi.fn(), add: vi.fn(), getCanvas: vi.fn(() => ({ _canvas: { captureStream: vi.fn(() => ({ getAudioTracks: vi.fn(() => []), addTrack: vi.fn(), removeTrack: vi.fn() })) } })) })),
    Animation: vi.fn(() => ({ start: vi.fn(), stop: vi.fn() })),
    Text: vi.fn(() => ({ x: vi.fn(), y: vi.fn(), destroy: vi.fn(), getLayer: vi.fn(() => null), visible: vi.fn(), opacity: vi.fn(), scaleX: vi.fn(), scaleY: vi.fn() })),
  },
}));

vi.mock("animejs", () => ({
  createTimeline: vi.fn(() => ({ add: vi.fn(), seek: vi.fn(), revert: vi.fn() })),
}));

vi.mock("pako", () => ({
  deflate: vi.fn((data) => new Uint8Array(data)),
  inflate: vi.fn((data) => data),
}));

vi.mock("../../../renderer/utils", () => ({
  getUid: vi.fn(() => "test-id"),
  isHtmlAudioElement: vi.fn((el) => el?.tagName === "AUDIO"),
  isHtmlImageElement: vi.fn((el) => el?.tagName === "IMG"),
  isHtmlVideoElement: vi.fn((el) => el?.tagName === "VIDEO"),
}));

vi.mock("../../../renderer/utils/konva-utils", () => ({
  KonvaAnimProxy: vi.fn().mockImplementation(() => ({})),
  KonvaUtils: { getClipRegion: vi.fn(() => ({ x: 0, y: 0, width: 100, height: 100 })) },
  getCoverCrop: vi.fn(() => ({ cropX: 0, cropY: 0, cropWidth: 100, cropHeight: 100 })),
  getFilterFromEffectType: vi.fn(() => "none"),
  makeImageSceneFunc: vi.fn(() => vi.fn()),
}));

vi.mock("@ffmpeg/ffmpeg", () => ({ FFmpeg: vi.fn() }));
vi.mock("@ffmpeg/util", () => ({ toBlobURL: vi.fn() }));
vi.mock("webm-duration-fix", () => ({ default: vi.fn() }));

Object.defineProperty(global, "window", {
  value: {
    addEventListener: vi.fn(),
    URL: { createObjectURL: vi.fn(() => "blob:test"), revokeObjectURL: vi.fn() },
    electron: { ipcRenderer: { invoke: vi.fn(), on: vi.fn(), off: vi.fn() } },
  },
  writable: true,
});

Object.defineProperty(global, "document", {
  value: {
    getElementById: vi.fn((id) => {
      if (id.startsWith("video-")) return { tagName: "VIDEO", src: "test-video-src", currentTime: 0, duration: 10, videoWidth: 1280, videoHeight: 720, play: vi.fn(), pause: vi.fn(), width: 0, height: 0, muted: false };
      if (id.startsWith("image-")) return { tagName: "IMG", src: "test-image-src", naturalWidth: 800, naturalHeight: 600, width: 0, height: 0 };
      if (id.startsWith("audio-")) return { tagName: "AUDIO", src: "test-audio-src", currentTime: 0, duration: 5, play: vi.fn(), pause: vi.fn(), id: "audio-123" };
      return null;
    }),
    createElement: vi.fn((type) => {
      if (type === "a") return { click: vi.fn(), download: "", href: "" };
      if (type === "video") return { srcObject: null, height: 0, width: 0, play: vi.fn(() => Promise.resolve()), remove: vi.fn() };
      return {};
    }),
  },
  writable: true,
});

class MockAudioContext {
  createMediaElementSource = vi.fn(() => ({ connect: vi.fn() }));
  createMediaStreamDestination = vi.fn(() => ({ stream: { getAudioTracks: vi.fn(() => [{ id: "test-audio-track" }]) } }));
}
global.AudioContext = MockAudioContext as any;

global.TextEncoder = class MockTextEncoder {
  encoding = "utf-8";
  encode(input: string) { return new Uint8Array([...input].map((c) => c.charCodeAt(0))); }
  encodeInto(s: string, d: Uint8Array) { const e = this.encode(s); d.set(e); return { read: s.length, written: e.length }; }
};
global.TextDecoder = class MockTextDecoder {
  encoding = "utf-8"; fatal = false; ignoreBOM = false;
  decode(input?: Uint8Array | ArrayBuffer) {
    if (!input) return "";
    const buf = input instanceof ArrayBuffer ? new Uint8Array(input) : input;
    return String.fromCharCode(...Array.from(buf));
  }
};

// ============================================================

describe("State Extended Tests", () => {
  let state: State;
  let mockStage: any;
  let mockLayer: any;

  beforeEach(() => {
    vi.clearAllMocks();
    mockLayer = { batchDraw: vi.fn(), add: vi.fn(), getCanvas: vi.fn(() => ({ getElement: vi.fn(() => ({ captureStream: vi.fn(() => ({ getAudioTracks: vi.fn(() => []), addTrack: vi.fn(), removeTrack: vi.fn() })) })) })) };
    mockStage = { width: vi.fn(), height: vi.fn(), container: vi.fn(() => ({ style: { backgroundColor: "" } })) };
    state = new State();
    state.setStage(mockStage, mockLayer, 800, 600);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("Advanced State Management", () => {
    it("should handle complex element operations", async () => {
      const element = {
        id: "test-id", name: "Test Element", type: "text",
        placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
        timeFrame: { start: 0, end: 1000 },
        properties: { text: "Test", fontSize: 16, fontWeight: 400, splittedTexts: [] },
      };

      await state.addEditorElement(element as any);
      expect(state.editorElements).toHaveLength(1);

      const updatedElement = { ...element, placement: { ...element.placement, x: 50 } };
      await state.updateEditorElement(updatedElement as any);
      expect(state.editorElements[0].placement.x).toBe(50);
    });

    it("should handle multiple elements", async () => {
      for (let i = 0; i < 5; i++) {
        await state.addEditorElement({
          id: `elem-${i}`, name: `Element ${i}`, type: "text",
          placement: { x: i * 10, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
          timeFrame: { start: 0, end: 1000 },
          properties: { text: `Text ${i}`, fontSize: 16, fontWeight: 400, splittedTexts: [] },
        } as any);
      }
      expect(state.editorElements).toHaveLength(5);
    });

    it("should maintain element order after updates", async () => {
      const elements = [
        { id: "a", name: "A", type: "text", placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 }, timeFrame: { start: 0, end: 1000 }, properties: { text: "A", fontSize: 16, fontWeight: 400, splittedTexts: [] } },
        { id: "b", name: "B", type: "text", placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 }, timeFrame: { start: 0, end: 1000 }, properties: { text: "B", fontSize: 16, fontWeight: 400, splittedTexts: [] } },
      ];
      for (const el of elements) {
        await state.addEditorElement(el as any);
      }
      expect(state.editorElements[0].id).toBe("a");
      expect(state.editorElements[1].id).toBe("b");
    });
  });

  describe("Animation System", () => {
    it("should add animations correctly", () => {
      const animation = { id: "anim-1", targetId: "elem-1", duration: 1000, type: "fadeIn" };
      state.addAnimation(animation as any);
      expect(state.animations).toHaveLength(1);
      expect(state.animations[0]).toMatchObject(animation);
    });

    it("should remove animations correctly", () => {
      const animation = { id: "anim-1", targetId: "elem-1", duration: 1000, type: "fadeIn" };
      state.addAnimation(animation as any);
      state.removeAnimation("anim-1");
      expect(state.animations).toHaveLength(0);
    });

    it("should update animations correctly", () => {
      const animation = { id: "anim-1", targetId: "elem-1", duration: 1000, type: "fadeIn" };
      state.addAnimation(animation as any);
      const updated = { ...animation, duration: 2000 };
      state.updateAnimation("anim-1", updated as any);
      expect(state.animations[0].duration).toBe(2000);
    });
  });

  describe("Playback Control", () => {
    it("should handle seek operations", () => {
      state.handleSeek(5000);
      expect(state.currentTimeInMs).toBe(5000);
      expect(state.playing).toBe(false);
    });

    it("should handle skip forward/backward", () => {
      state.handleSeek(15000);
      state.skipForward();
      expect(state.currentTimeInMs).toBe(25000);

      state.skipBackward();
      expect(state.currentTimeInMs).toBe(15000);
    });

    it("should clamp to maxTime when skipping forward past end", () => {
      state.handleSeek(state.maxTime - 5000);
      state.skipForward();
      expect(state.currentTimeInMs).toBe(state.maxTime);
    });

    it("should clamp to 0 when skipping backward past start", () => {
      state.handleSeek(5000);
      state.skipBackward();
      expect(state.currentTimeInMs).toBe(0);
    });
  });

  describe("Clipboard Operations", () => {
    it("should copy and paste text elements", async () => {
      const element = {
        id: "text-1", name: "Text 1", type: "text",
        placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
        timeFrame: { start: 0, end: 1000 },
        properties: { text: "Hello", fontSize: 16, fontWeight: 400, splittedTexts: [] },
      };
      await state.addEditorElement(element as any);
      state.setSelectedElement(element as any);
      state.copyObject();
      expect(state.clipboard?.id).toBe("text-1");
    });

    it("should not paste when clipboard is empty", () => {
      state.elementStore.clipboard = null;
      const spy = vi.spyOn(state.elementStore, "addText");
      state.pasteObject();
      expect(spy).not.toHaveBeenCalled();
    });
  });

  describe("Canvas Size", () => {
    it("setCanvasSize updates dimensions", () => {
      state.setCanvasSize(1920, 1080);
      expect(state.canvas_width).toBe(1920);
      expect(state.canvas_height).toBe(1080);
    });
  });

  describe("Background Color", () => {
    it("should set background color", () => {
      state.setBackgroundColor("#ff0000");
      expect(state.backgroundColor).toBe("#ff0000");
    });
  });

  describe("Media Resources", () => {
    it("should add and track video resources", () => {
      state.addVideoResource("video-url-1");
      state.addVideoResource("video-url-2");
      expect(state.videos).toContain("video-url-1");
      expect(state.videos).toContain("video-url-2");
    });

    it("should add and track audio resources", () => {
      state.addAudioResource("audio-url");
      expect(state.audios).toContain("audio-url");
    });

    it("should add and track image resources", () => {
      state.addImageResource("image-url");
      expect(state.images).toContain("image-url");
    });
  });

  describe("Video Format", () => {
    it("should update format to webm", () => {
      state.setVideoFormat("webm");
      expect(state.selectedVideoFormat).toBe("webm");
    });
    it("should update format to mp4", () => {
      state.setVideoFormat("webm");
      state.setVideoFormat("mp4");
      expect(state.selectedVideoFormat).toBe("mp4");
    });
  });

  describe("Project Loading Status", () => {
    it("should cycle through loading states", () => {
      state.setProjectLoadingStatus("loading", "Loading...");
      expect(state.projectLoadingStatus).toBe("loading");
      expect(state.projectLoadingMessage).toBe("Loading...");

      state.setProjectLoadingStatus("success", "Done");
      expect(state.projectLoadingStatus).toBe("success");

      state.setProjectLoadingStatus("idle");
      expect(state.projectLoadingStatus).toBe("idle");
    });

    it("should update progress", () => {
      state.setProjectLoadingProgress(50);
      expect(state.projectLoadingProgress).toBe(50);
    });
  });

  describe("updateEditorElementTimeFrame", () => {
    it("should update the time frame of an element", async () => {
      const element = {
        id: "elem-tf", name: "E", type: "text",
        placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
        timeFrame: { start: 0, end: 1000 },
        properties: { text: "T", fontSize: 16, fontWeight: 400, splittedTexts: [] },
      };
      await state.addEditorElement(element as any);
      await state.updateEditorElementTimeFrame(element as any, { start: 500 });
      const updated = state.editorElements.find((e) => e.id === "elem-tf");
      expect(updated?.timeFrame.start).toBe(500);
    });
  });
});
