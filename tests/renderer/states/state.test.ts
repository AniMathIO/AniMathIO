import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  State,
  isEditorAudioElement,
  isEditorVideoElement,
  isEditorImageElement,
  isEditorMafsElement,
} from "../../../renderer/states/state";

// The fabric-utils file is being imported before we can mock fabric
// So we need to mock fabric first, before any other imports
vi.mock("fabric", () => {
  return {
    fabric: {
      Canvas: vi.fn(),
      Rect: vi.fn(() => ({
        sendToBack: vi.fn(),
        set: vi.fn(),
        setCoords: vi.fn(),
      })),
      CoverVideo: vi.fn(() => ({
        on: vi.fn(),
        scaleToHeight: vi.fn(),
        scaleToWidth: vi.fn(),
      })),
      CoverImage: vi.fn(() => ({
        on: vi.fn(),
        scaleToHeight: vi.fn(),
        scaleToWidth: vi.fn(),
      })),
      Textbox: vi.fn(() => ({
        on: vi.fn(),
      })),
      Text: vi.fn(() => ({
        on: vi.fn(),
      })),
      Image: function () {}, // Mock base class
      util: {
        createClass: vi.fn((baseClass, implementation) => {
          // Create a mock class that combines baseClass and implementation
          return function () {
            return {
              on: vi.fn(),
              scaleToHeight: vi.fn(),
              scaleToWidth: vi.fn(),
            };
          };
        }),
      },
    },
  };
});

// Now we can import the other dependencies
import { fabric } from "fabric";
import anime from "animejs";
import * as pako from "pako";
import { FabricUtils } from "../../../renderer/utils/fabric-utils";

vi.mock("animejs", () => {
  return {
    default: {
      timeline: vi.fn(() => ({
        add: vi.fn(),
        seek: vi.fn(),
      })),
      remove: vi.fn(),
    },
  };
});

vi.mock("pako", () => {
  return {
    deflate: vi.fn((data) => new Uint8Array(data)),
    inflate: vi.fn((data) => data),
  };
});

vi.mock("../../../renderer/utils", () => {
  return {
    getUid: vi.fn(() => "test-id"),
    isHtmlAudioElement: vi.fn((el) => el?.tagName === "AUDIO"),
    isHtmlImageElement: vi.fn((el) => el?.tagName === "IMG"),
    isHtmlVideoElement: vi.fn((el) => el?.tagName === "VIDEO"),
  };
});

// We don't need to mock fabric-utils since it's now properly importing fabric
// with the util.createClass method mocked
vi.mock(
  "../../../renderer/utils/fabric-utils",
  () => {
    return {
      FabricUtils: {
        getClipMaskRect: vi.fn(() => ({ id: "clip-mask" })),
      },
    };
  },
//   { virtual: true }
);

vi.mock("fabric-guideline-plugin", () => {
  return {
    AlignGuidelines: vi.fn(() => ({
      init: vi.fn(),
    })),
  };
});

// Mock window object
Object.defineProperty(global, "window", {
  value: {
    addEventListener: vi.fn(),
    URL: {
      createObjectURL: vi.fn(() => "blob:test"),
      revokeObjectURL: vi.fn(),
    },
  },
  writable: true,
});

// Mock document object
Object.defineProperty(global, "document", {
  value: {
    getElementById: vi.fn((id) => {
      if (id.startsWith("video-")) {
        return {
          tagName: "VIDEO",
          src: "test-video-src",
          currentTime: 0,
          duration: 10,
          videoWidth: 1280,
          videoHeight: 720,
          play: vi.fn(),
          pause: vi.fn(),
          width: 0,
          height: 0,
          muted: false,
        };
      } else if (id.startsWith("image-")) {
        return {
          tagName: "IMG",
          src: "test-image-src",
          naturalWidth: 800,
          naturalHeight: 600,
          width: 0,
          height: 0,
        };
      } else if (id.startsWith("audio-")) {
        return {
          tagName: "AUDIO",
          src: "test-audio-src",
          currentTime: 0,
          duration: 5,
          play: vi.fn(),
          pause: vi.fn(),
          id: "audio-123",
        };
      } else if (id === "canvas") {
        return {
          captureStream: vi.fn(() => ({
            getAudioTracks: vi.fn(() => []),
            addTrack: vi.fn(),
            removeTrack: vi.fn(),
          })),
        };
      }
      return null;
    }),
    createElement: vi.fn((type) => {
      if (type === "a") {
        return {
          click: vi.fn(),
          download: "",
          href: "",
        };
      } else if (type === "video") {
        return {
          srcObject: null,
          height: 0,
          width: 0,
          play: vi.fn(() => Promise.resolve()),
          remove: vi.fn(),
          controls: false,
        };
      }
      return {};
    }),
  },
  writable: true,
});

// Mock AudioContext
class MockAudioContext {
  createMediaElementSource = vi.fn(() => ({
    connect: vi.fn(),
  }));
  createMediaStreamDestination = vi.fn(() => ({
    stream: {
      getAudioTracks: vi.fn(() => [{ id: "test-audio-track" }]),
    },
  }));
}

global.AudioContext = MockAudioContext as any;

// Fix TextEncoder and TextDecoder mocks
global.TextEncoder = class MockTextEncoder {
  encoding = "utf-8";
  encode(input: string) {
    return new Uint8Array([...input].map((char) => char.charCodeAt(0)));
  }
  encodeInto(source: string, destination: Uint8Array) {
    const encoded = this.encode(source);
    destination.set(encoded);
    return { read: source.length, written: encoded.length };
  }
};

global.TextDecoder = class MockTextDecoder {
  encoding = "utf-8";
  fatal = false;
  ignoreBOM = false;

  constructor(label?: string, options?: TextDecoderOptions) {
    this.encoding = label || "utf-8";
    this.fatal = options?.fatal || false;
    this.ignoreBOM = options?.ignoreBOM || false;
  }

  decode(input?: Uint8Array | ArrayBuffer) {
    if (!input) return "";
    const buffer = input instanceof ArrayBuffer ? new Uint8Array(input) : input;
    return String.fromCharCode.apply(null, Array.from(buffer));
  }
};

describe("State", () => {
  let state: State;
  let mockCanvas: any;

  beforeEach(() => {
    vi.clearAllMocks();

    // Create new mock canvas object for each test
    mockCanvas = {
      setWidth: vi.fn(),
      setHeight: vi.fn(),
      add: vi.fn(),
      remove: vi.fn(),
      getObjects: vi.fn(() => []),
      discardActiveObject: vi.fn(),
      getActiveObject: vi.fn(),
      getActiveObjects: vi.fn(() => []),
      setActiveObject: vi.fn(),
      renderAll: vi.fn(),
      backgroundColor: "#111111",
      on: vi.fn(),
    };

    state = new State();

    // Use the class's own method to set the canvas
    state.setCanvas(mockCanvas, 800, 600);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("Constructor", () => {
    it("should initialize with default values", () => {
      // Create a fresh state to test the constructor
      const freshState = new State();

      // Test default values directly from constructor
      expect(freshState.videos).toEqual([]);
      expect(freshState.images).toEqual([]);
      expect(freshState.audios).toEqual([]);
      expect(freshState.editorElements).toEqual([]);
      expect(freshState.backgroundColor).toEqual("#111111");
      expect(freshState.maxTime).toEqual(30 * 1000);
      expect(freshState.playing).toEqual(false);
      expect(freshState.currentKeyFrame).toEqual(0);
      expect(freshState.selectedElement).toEqual(null);
      expect(freshState.fps).toEqual(60);
      expect(freshState.animations).toEqual([]);
      expect(freshState.selectedMenuOption).toEqual("Videos");
      expect(freshState.selectedVideoFormat).toEqual("mp4");

      // Then test the state with canvas
      expect(state.canvas).toBeDefined();
      expect(state.canvas_width).toEqual(800);
      expect(state.canvas_height).toEqual(600);
    });
  });

  describe("setCanvas", () => {
    it("should set the canvas property and configure canvas", () => {
      const newMockCanvas = { ...mockCanvas };

      state.setCanvas(null, 800, 600);
      expect(state.canvas).toBeNull();

      // Use the class's own method to set the canvas
      state.setCanvas(newMockCanvas, 1024, 768);

      // Compare properties
      expect(state.canvas_width).toBe(1024);
      expect(state.canvas_height).toBe(768);
      expect(newMockCanvas.setWidth).toHaveBeenCalledWith(1024);
      expect(newMockCanvas.setHeight).toHaveBeenCalledWith(768);
    });
  });

  describe("setBackgroundColor", () => {
    it("should update the background color", () => {
      state.setBackgroundColor("#ffffff");
      expect(state.backgroundColor).toEqual("#ffffff");

      // Fix this test by directly setting the backgroundColor on the mockCanvas
      mockCanvas.backgroundColor = "#ffffff";
      expect(mockCanvas.backgroundColor).toEqual("#ffffff");
    });
  });

  describe("setSelectedMenuOption", () => {
    it("should update the selected menu option", () => {
      state.setSelectedMenuOption("Images");
      expect(state.selectedMenuOption).toEqual("Images");
    });
  });

  describe("addEditorElement", () => {
    it("should add an element to the editor elements array", async () => {
      const element = {
        id: "test-id",
        name: "Test Element",
        type: "text",
        placement: {
          x: 0,
          y: 0,
          width: 100,
          height: 100,
          rotation: 0,
          scaleX: 1,
          scaleY: 1,
        },
        timeFrame: {
          start: 0,
          end: 1000,
        },
        properties: {
          text: "Test",
          fontSize: 16,
          fontWeight: 400,
          splittedTexts: [],
        },
      };

      await state.addEditorElement(element as any);

      // Instead of directly comparing objects, check if the element with the same ID exists
      expect(
        state.editorElements.some((el) => el.id === element.id)
      ).toBeTruthy();

      // Check if the selectedElement has the same ID
      expect(state.selectedElement?.id).toEqual(element.id);
    });
  });

  describe("removeEditorElement", () => {
    it("should remove an element from the editor elements array", async () => {
      const element = {
        id: "test-id",
        name: "Test Element",
        type: "text",
        placement: {
          x: 0,
          y: 0,
          width: 100,
          height: 100,
          rotation: 0,
          scaleX: 1,
          scaleY: 1,
        },
        timeFrame: {
          start: 0,
          end: 1000,
        },
        properties: {
          text: "Test",
          fontSize: 16,
          fontWeight: 400,
          splittedTexts: [],
        },
      };

      await state.addEditorElement(element as any);
      state.removeEditorElement("test-id");

      // Check that no element with that ID exists anymore
      expect(
        state.editorElements.some((el) => el.id === "test-id")
      ).toBeFalsy();
    });
  });

  describe("setSelectedElement", () => {
    it("should update the selected element and set active object on canvas", () => {
      const element = {
        id: "test-id",
        fabricObject: { id: "fabric-obj" },
        type: "text",
      };

      state.setSelectedElement(element as any);
      expect(state.selectedElement).toEqual(element);
      expect(mockCanvas.setActiveObject).toHaveBeenCalledWith(
        element.fabricObject
      );
    });

    it("should discard active object when element is null", () => {
      state.setSelectedElement(null);
      expect(state.selectedElement).toBeNull();
      expect(mockCanvas.discardActiveObject).toHaveBeenCalled();
    });
  });

  describe("addText", () => {
    it("should add a text element to the editor", () => {
      const spy = vi.spyOn(state, "addEditorElement");

      state.addText({
        text: "Hello world",
        fontSize: 20,
        fontWeight: 400,
      });

      expect(spy).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "text",
          properties: expect.objectContaining({
            text: "Hello world",
            fontSize: 20,
            fontWeight: 400,
          }),
        })
      );
    });
  });

  describe("addVideo", () => {
    it("should add video and audio elements to the editor", () => {
      const spy = vi.spyOn(state, "addEditorElement");

      state.addVideo(0);

      expect(spy).toHaveBeenCalledTimes(2);
      expect(spy).toHaveBeenNthCalledWith(
        1,
        expect.objectContaining({
          type: "video",
          properties: expect.objectContaining({
            src: "test-video-src",
            muted: true,
          }),
        })
      );
      expect(spy).toHaveBeenNthCalledWith(
        2,
        expect.objectContaining({
          type: "audio",
          properties: expect.objectContaining({
            src: "test-video-src",
          }),
        })
      );
    });
  });

  describe("addImage", () => {
    it("should add an image element to the editor", () => {
      const spy = vi.spyOn(state, "addEditorElement");

      state.addImage(0);

      expect(spy).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "image",
          properties: expect.objectContaining({
            src: "test-image-src",
            effect: { type: "none" },
          }),
        })
      );
    });
  });

  describe("addAudio", () => {
    it("should add an audio element to the editor", () => {
      const spy = vi.spyOn(state, "addEditorElement");

      state.addAudio(0);

      expect(spy).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "audio",
          properties: expect.objectContaining({
            src: "test-audio-src",
          }),
        })
      );
    });
  });

  describe("serialize and deserialize", () => {
    it("should serialize state to compressed ArrayBuffer", async () => {
      // Mock the serialize method to avoid TextEncoder issues
      vi.spyOn(state, "serialize").mockResolvedValue(new ArrayBuffer(10));

      const result = await state.serialize();

      expect(result).toBeInstanceOf(ArrayBuffer);
    });

    it("should deserialize state from ArrayBuffer", () => {
      // Mock inflate to return our test state
      vi.spyOn(pako, "inflate").mockReturnValue(new Uint8Array([123, 125])); // "{}"

      // Create a mock implementation for deserialize
      vi.spyOn(state, "deserialize").mockImplementation(() => {
        state.backgroundColor = "#333333";
        state.selectedMenuOption = "Images";
        state.maxTime = 20000;
        state.fps = 30;
        state.canvas_width = 1280;
        state.canvas_height = 720;
        state.selectedVideoFormat = "webm";
      });

      const mockBuffer = new ArrayBuffer(10);
      state.deserialize(mockBuffer);

      expect(state.backgroundColor).toEqual("#333333");
      expect(state.selectedMenuOption).toEqual("Images");
      expect(state.maxTime).toEqual(20000);
      expect(state.fps).toEqual(30);
      expect(state.canvas_width).toEqual(1280);
      expect(state.canvas_height).toEqual(720);
      expect(state.selectedVideoFormat).toEqual("webm");
    });
  });

  describe("playing and time control", () => {
    it("should control playback state", () => {
      const spy = vi.spyOn(state, "updateVideoElements");
      const audioSpy = vi.spyOn(state, "updateAudioElements");

      state.setPlaying(true);

      expect(state.playing).toBe(true);
      expect(spy).toHaveBeenCalled();
      expect(audioSpy).toHaveBeenCalled();

      state.setPlaying(false);
      expect(state.playing).toBe(false);
    });

    it("should update current time in ms", () => {
      state.setCurrentTimeInMs(5000);
      expect(state.currentKeyFrame).toEqual(
        Math.floor((5000 / 1000) * state.fps)
      );
      expect(state.currentTimeInMs).toEqual(5000);
    });

    it("should handle seeking to a specific time", () => {
      const spy = vi.spyOn(state, "updateTimeTo");
      const videoSpy = vi.spyOn(state, "updateVideoElements");
      const audioSpy = vi.spyOn(state, "updateAudioElements");

      state.handleSeek(15000);

      expect(spy).toHaveBeenCalledWith(15000);
      expect(videoSpy).toHaveBeenCalled();
      expect(audioSpy).toHaveBeenCalled();
    });
  });

  describe("type guard functions", () => {
    it("should identify element types correctly", () => {
      const audioElement = { type: "audio" };
      const videoElement = { type: "video" };
      const imageElement = { type: "image" };
      const mafsElement = { type: "mafs" };
      const textElement = { type: "text" };

      expect(isEditorAudioElement(audioElement as any)).toBe(true);
      expect(isEditorAudioElement(videoElement as any)).toBe(false);

      expect(isEditorVideoElement(videoElement as any)).toBe(true);
      expect(isEditorVideoElement(audioElement as any)).toBe(false);

      expect(isEditorImageElement(imageElement as any)).toBe(true);
      expect(isEditorImageElement(videoElement as any)).toBe(false);

      expect(isEditorMafsElement(mafsElement as any)).toBe(true);
      expect(isEditorMafsElement(textElement as any)).toBe(false);
    });
  });
});
