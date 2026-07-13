import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  State,
  isEditorAudioElement,
  isEditorVideoElement,
  isEditorImageElement,
  isEditorMafsElement,
} from "../../../renderer/states/state";

// Mock Konva before any imports
vi.mock("konva", () => {
  const mockLayer = {
    batchDraw: vi.fn(),
    add: vi.fn(),
    getCanvas: vi.fn(() => ({ _canvas: { captureStream: vi.fn(() => ({ getAudioTracks: vi.fn(() => []), addTrack: vi.fn(), removeTrack: vi.fn() })) } })),
  };
  const mockStage = {
    width: vi.fn(),
    height: vi.fn(),
    container: vi.fn(() => ({ style: { backgroundColor: "" }, querySelector: vi.fn() })),
  };
  return {
    default: {
      Stage: vi.fn(() => mockStage),
      Layer: vi.fn(() => mockLayer),
      Animation: vi.fn(() => ({ start: vi.fn(), stop: vi.fn() })),
      Text: vi.fn(() => ({ x: vi.fn(), y: vi.fn(), destroy: vi.fn(), getLayer: vi.fn(() => null), visible: vi.fn(), opacity: vi.fn(), scaleX: vi.fn(), scaleY: vi.fn() })),
    },
  };
});

vi.mock("animejs", () => ({
  createTimeline: vi.fn(() => ({
    add: vi.fn(),
    seek: vi.fn(),
    revert: vi.fn(),
  })),
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

// Mock window
Object.defineProperty(global, "window", {
  value: { addEventListener: vi.fn(), URL: { createObjectURL: vi.fn(() => "blob:test"), revokeObjectURL: vi.fn() } },
  writable: true,
});

// Mock document
Object.defineProperty(global, "document", {
  value: {
    getElementById: vi.fn((id) => {
      if (id.startsWith("video-")) {
        return { tagName: "VIDEO", src: "test-video-src", currentTime: 0, duration: 10, videoWidth: 1280, videoHeight: 720, play: vi.fn(), pause: vi.fn(), width: 0, height: 0, muted: false };
      } else if (id.startsWith("image-")) {
        return { tagName: "IMG", src: "test-image-src", naturalWidth: 800, naturalHeight: 600, width: 0, height: 0 };
      } else if (id.startsWith("audio-")) {
        return { tagName: "AUDIO", src: "test-audio-src", currentTime: 0, duration: 5, play: vi.fn(), pause: vi.fn(), id: "audio-123" };
      }
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
  encodeInto(source: string, dest: Uint8Array) { const e = this.encode(source); dest.set(e); return { read: source.length, written: e.length }; }
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

describe("State (RootStore)", () => {
  let state: State;
  let mockStage: any;
  let mockLayer: any;

  beforeEach(() => {
    vi.clearAllMocks();

    mockLayer = {
      batchDraw: vi.fn(),
      add: vi.fn(),
      getCanvas: vi.fn(() => ({
        getElement: vi.fn(() => ({
          captureStream: vi.fn(() => ({
            getAudioTracks: vi.fn(() => []),
            addTrack: vi.fn(),
            removeTrack: vi.fn(),
          })),
        })),
      })),
    };

    mockStage = {
      width: vi.fn(),
      height: vi.fn(),
      container: vi.fn(() => ({ style: { backgroundColor: "" }, querySelector: vi.fn() })),
    };

    state = new State();
    state.setStage(mockStage, mockLayer, 800, 600);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // ---- Constructor defaults ----
  describe("Constructor", () => {
    it("should initialize with default values", () => {
      const fresh = new State();
      expect(fresh.videos).toEqual([]);
      expect(fresh.images).toEqual([]);
      expect(fresh.audios).toEqual([]);
      expect(fresh.editorElements).toEqual([]);
      expect(fresh.backgroundColor).toEqual("#111111");
      expect(fresh.maxTime).toEqual(30 * 1000);
      expect(fresh.playing).toEqual(false);
      expect(fresh.currentKeyFrame).toEqual(0);
      expect(fresh.selectedElement).toEqual(null);
      expect(fresh.fps).toEqual(60);
      expect(fresh.animations).toEqual([]);
      expect(fresh.selectedMenuOption).toEqual("Videos");
      expect(fresh.selectedVideoFormat).toEqual("mp4");
    });

    it("should have canvas_width/height after setStage", () => {
      expect(state.canvas_width).toEqual(800);
      expect(state.canvas_height).toEqual(600);
    });
  });

  // ---- setStage / canvas ----
  describe("setStage", () => {
    it("should set stage and canvas dimensions", () => {
      state.setStage(mockStage, mockLayer, 1024, 768);
      expect(state.canvas_width).toBe(1024);
      expect(state.canvas_height).toBe(768);
      expect(mockStage.width).toHaveBeenCalledWith(1024);
      expect(mockStage.height).toHaveBeenCalledWith(768);
    });

    it("should handle null stage", () => {
      state.setStage(null, null, 800, 600);
      expect(state.stage).toBeNull();
    });
  });

  // ---- Background color ----
  describe("setBackgroundColor", () => {
    it("should update backgroundColor", () => {
      state.setBackgroundColor("#ffffff");
      expect(state.backgroundColor).toEqual("#ffffff");
    });
  });

  // ---- Menu option ----
  describe("setSelectedMenuOption", () => {
    it("should update the selected menu option", () => {
      state.setSelectedMenuOption("Images");
      expect(state.selectedMenuOption).toEqual("Images");
    });
  });

  // ---- Element management ----
  describe("addEditorElement", () => {
    it("should add an element to the editor elements array", async () => {
      const element = {
        id: "test-id", name: "Test Element", type: "text",
        placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
        timeFrame: { start: 0, end: 1000 },
        properties: { text: "Test", fontSize: 16, fontWeight: 400, splittedTexts: [] },
      };
      await state.addEditorElement(element as any);
      expect(state.editorElements.some((el) => el.id === element.id)).toBeTruthy();
      expect(state.selectedElement?.id).toEqual(element.id);
    });
  });

  describe("removeEditorElement", () => {
    it("should remove an element from the editor elements array", async () => {
      const element = {
        id: "test-id", name: "Test", type: "text",
        placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
        timeFrame: { start: 0, end: 1000 },
        properties: { text: "Test", fontSize: 16, fontWeight: 400, splittedTexts: [] },
      };
      await state.addEditorElement(element as any);
      state.removeEditorElement("test-id");
      expect(state.editorElements.some((el) => el.id === "test-id")).toBeFalsy();
    });
  });

  describe("setSelectedElement", () => {
    it("should update the selected element", () => {
      const element = { id: "test-id", type: "text" };
      state.setSelectedElement(element as any);
      expect(state.selectedElement).toEqual(element);
    });

    it("should clear selected element when null", () => {
      state.setSelectedElement(null);
      expect(state.selectedElement).toBeNull();
    });
  });

  // ---- addText ----
  describe("addText", () => {
    it("should add a text element to the editor", () => {
      const spy = vi.spyOn(state.elementStore, "addEditorElement");
      state.addText({ text: "Hello world", fontSize: 20, fontWeight: 400 });
      expect(spy).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "text",
          properties: expect.objectContaining({ text: "Hello world", fontSize: 20, fontWeight: 400 }),
        })
      );
    });

    it("without a dropPosition, places the element at (0, 0) (unchanged click-to-add behavior)", () => {
      const spy = vi.spyOn(state.elementStore, "addEditorElement");
      state.addText({ text: "Hello world", fontSize: 20, fontWeight: 400 });
      const placement = spy.mock.calls[0][0].placement;
      expect(placement.x).toBe(0);
      expect(placement.y).toBe(0);
    });

    it("with a dropPosition, centers the element on the drop point", () => {
      const spy = vi.spyOn(state.elementStore, "addEditorElement");
      state.addText({ text: "Hello world", fontSize: 20, fontWeight: 400 }, { x: 500, y: 300 });
      const placement = spy.mock.calls[0][0].placement;
      // default text placement is 100x100, so centering subtracts half that
      expect(placement.x).toBe(450);
      expect(placement.y).toBe(250);
    });
  });

  // ---- addVideo ----
  describe("addVideo", () => {
    it("should add video and audio elements to the editor", () => {
      const spy = vi.spyOn(state.elementStore, "addEditorElement");
      state.addVideo(0);
      expect(spy).toHaveBeenCalledTimes(2);
      expect(spy).toHaveBeenNthCalledWith(1, expect.objectContaining({ type: "video" }));
      expect(spy).toHaveBeenNthCalledWith(2, expect.objectContaining({ type: "audio" }));
    });

    it("without a dropPosition, places the video element at (0, 0)", () => {
      const spy = vi.spyOn(state.elementStore, "addEditorElement");
      state.addVideo(0);
      const placement = spy.mock.calls[0][0].placement;
      expect(placement.x).toBe(0);
      expect(placement.y).toBe(0);
    });

    it("with a dropPosition, centers the video element on the drop point", () => {
      const spy = vi.spyOn(state.elementStore, "addEditorElement");
      state.addVideo(0, { x: 500, y: 300 });
      const placement = spy.mock.calls[0][0].placement;
      // video-0 mock is 1280x720 -> width = 100 * (1280/720)
      const expectedWidth = 100 * (1280 / 720);
      expect(placement.x).toBeCloseTo(500 - expectedWidth / 2);
      expect(placement.y).toBeCloseTo(300 - 50);
    });
  });

  // ---- addImage ----
  describe("addImage", () => {
    it("should add an image element to the editor", () => {
      const spy = vi.spyOn(state.elementStore, "addEditorElement");
      state.addImage(0);
      expect(spy).toHaveBeenCalledWith(expect.objectContaining({ type: "image" }));
    });

    it("without a dropPosition, places the image element at (0, 0)", () => {
      const spy = vi.spyOn(state.elementStore, "addEditorElement");
      state.addImage(0);
      const placement = spy.mock.calls[0][0].placement;
      expect(placement.x).toBe(0);
      expect(placement.y).toBe(0);
    });

    it("with a dropPosition, centers the image element on the drop point", () => {
      const spy = vi.spyOn(state.elementStore, "addEditorElement");
      state.addImage(0, { x: 500, y: 300 });
      const placement = spy.mock.calls[0][0].placement;
      // image-0 mock is 800x600 -> width = 100 * (800/600)
      const expectedWidth = 100 * (800 / 600);
      expect(placement.x).toBeCloseTo(500 - expectedWidth / 2);
      expect(placement.y).toBeCloseTo(300 - 50);
    });
  });

  // ---- addAudio ----
  describe("addAudio", () => {
    it("should add an audio element to the editor", () => {
      const spy = vi.spyOn(state.elementStore, "addEditorElement");
      state.addAudio(0);
      expect(spy).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "audio",
          properties: expect.objectContaining({ volume: 1, muted: false }),
        })
      );
    });
  });

  // ---- playback ----
  describe("setPlaying", () => {
    it("should update the playing state", () => {
      state.setPlaying(true);
      expect(state.playing).toEqual(true);
      state.setPlaying(false);
      expect(state.playing).toEqual(false);
    });
  });

  describe("currentTimeInMs", () => {
    it("should return current time in ms based on keyframe and fps", () => {
      state.playback.currentKeyFrame = 60;
      state.playback.fps = 60;
      expect(state.currentTimeInMs).toEqual(1000);
    });
  });

  // ---- setCanvasSize ----
  describe("setCanvasSize", () => {
    it("should update canvas_width and canvas_height", () => {
      state.setCanvasSize(1920, 1080);
      expect(state.canvas_width).toEqual(1920);
      expect(state.canvas_height).toEqual(1080);
    });

    it("should proportionally rescale element placements", async () => {
      const element = {
        id: "elem-1", name: "Test", type: "text",
        placement: { x: 400, y: 300, width: 200, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
        timeFrame: { start: 0, end: 1000 },
        properties: { text: "T", fontSize: 16, fontWeight: 400, splittedTexts: [] },
      };
      await state.addEditorElement(element as any);
      state.setCanvasSize(1600, 1200); // 2x both dimensions
      const updated = state.editorElements.find((e) => e.id === "elem-1");
      // old canvas was 800x600, new is 1600x1200 → 2x scale
      expect(updated?.placement.x).toBeCloseTo(800);
      expect(updated?.placement.y).toBeCloseTo(600);
    });
  });

  // ---- animations ----
  describe("addAnimation", () => {
    it("should add an animation", () => {
      const animation = { id: "anim-1", targetId: "elem-1", duration: 500, type: "fadeIn" };
      state.addAnimation(animation as any);
      expect(state.animations.some((a) => a.id === "anim-1")).toBeTruthy();
    });
  });

  describe("removeAnimation", () => {
    it("should remove an animation", () => {
      const animation = { id: "anim-1", targetId: "elem-1", duration: 500, type: "fadeIn" };
      state.addAnimation(animation as any);
      state.removeAnimation("anim-1");
      expect(state.animations.some((a) => a.id === "anim-1")).toBeFalsy();
    });
  });

  // ---- Media resources ----
  describe("addVideoResource", () => {
    it("should add a video resource", () => {
      state.addVideoResource("test-video-url");
      expect(state.videos).toContain("test-video-url");
    });
  });

  describe("addImageResource", () => {
    it("should add an image resource", () => {
      state.addImageResource("test-image-url");
      expect(state.images).toContain("test-image-url");
    });
  });

  describe("addAudioResource", () => {
    it("should add an audio resource", () => {
      state.addAudioResource("test-audio-url");
      expect(state.audios).toContain("test-audio-url");
    });
  });

  // ---- Type guards ----
  describe("type guard functions", () => {
    it("isEditorAudioElement", () => {
      expect(isEditorAudioElement({ type: "audio" } as any)).toBe(true);
      expect(isEditorAudioElement({ type: "video" } as any)).toBe(false);
    });
    it("isEditorVideoElement", () => {
      expect(isEditorVideoElement({ type: "video" } as any)).toBe(true);
      expect(isEditorVideoElement({ type: "audio" } as any)).toBe(false);
    });
    it("isEditorImageElement", () => {
      expect(isEditorImageElement({ type: "image" } as any)).toBe(true);
      expect(isEditorImageElement({ type: "text" } as any)).toBe(false);
    });
    it("isEditorMafsElement", () => {
      expect(isEditorMafsElement({ type: "mafs" } as any)).toBe(true);
      expect(isEditorMafsElement({ type: "image" } as any)).toBe(false);
    });
  });

  // ---- Project state ----
  describe("project state", () => {
    it("setCurrentProjectFilePath", () => {
      state.setCurrentProjectFilePath("/path/to/file");
      expect(state.currentProjectFilePath).toBe("/path/to/file");
    });
    it("setCurrentProjectFileName", () => {
      state.setCurrentProjectFileName("my-project.amo");
      expect(state.currentProjectFileName).toBe("my-project.amo");
    });
    it("setEditorActive", () => {
      state.setEditorActive(true);
      expect(state.isEditorActive).toBe(true);
    });
    it("setProjectLoadingStatus", () => {
      state.setProjectLoadingStatus("loading", "Loading project...");
      expect(state.projectLoadingStatus).toBe("loading");
      expect(state.projectLoadingMessage).toBe("Loading project...");
    });
  });

  // ---- Video format ----
  describe("setVideoFormat", () => {
    it("should update the selected video format", () => {
      state.setVideoFormat("webm");
      expect(state.selectedVideoFormat).toBe("webm");
    });
  });

  // ---- handleSeek ----
  describe("handleSeek", () => {
    it("should stop playing and update time", () => {
      state.setPlaying(true);
      state.handleSeek(5000);
      expect(state.playing).toBe(false);
      expect(state.currentTimeInMs).toBe(5000);
    });
  });

  // ---- updateAudioSettings ----
  describe("updateAudioSettings", () => {
    it("should update volume and muted for an audio element", async () => {
      const element = {
        id: "audio-elem", name: "Audio", type: "audio",
        placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
        timeFrame: { start: 0, end: 5000 },
        properties: { elementId: "audio-audio-elem", src: "test.mp3", volume: 1, muted: false },
      };
      await state.addEditorElement(element as any);
      state.updateAudioSettings("audio-elem", { volume: 0.5, muted: true });
      const updated = state.editorElements.find((e) => e.id === "audio-elem");
      expect((updated as any).properties.volume).toBe(0.5);
      expect((updated as any).properties.muted).toBe(true);
    });
  });

  // ---- copyObject / pasteObject ----
  describe("copyObject / pasteObject", () => {
    it("should copy and paste a text element", async () => {
      const element = {
        id: "text-1", name: "Text 1", type: "text",
        placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
        timeFrame: { start: 0, end: 1000 },
        properties: { text: "Hello", fontSize: 16, fontWeight: 400, splittedTexts: [] },
      };
      await state.addEditorElement(element as any);
      state.setSelectedElement(element as any);
      state.copyObject();
      expect(state.clipboard).toEqual(element);

      const spy = vi.spyOn(state.elementStore, "addText");
      state.pasteObject();
      expect(spy).toHaveBeenCalledWith({ text: "Hello", fontSize: 16, fontWeight: 400 });
    });
  });

  // ---- deleteSelectedObjects ----
  describe("deleteSelectedObjects", () => {
    it("should delete selected elements", async () => {
      const element = {
        id: "del-1", name: "Del", type: "text",
        placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
        timeFrame: { start: 0, end: 1000 },
        properties: { text: "Del", fontSize: 16, fontWeight: 400, splittedTexts: [] },
      };
      await state.addEditorElement(element as any);
      state.deleteSelectedObjects([element] as any);
      expect(state.editorElements.some((e) => e.id === "del-1")).toBeFalsy();
    });
  });
});
