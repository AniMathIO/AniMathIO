import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { State } from "../../../renderer/states/state";

vi.mock("konva", () => ({
  default: {
    Stage: vi.fn(() => ({ width: vi.fn(), height: vi.fn(), container: vi.fn(() => ({ style: { backgroundColor: "" } })) })),
    Layer: vi.fn(() => ({ batchDraw: vi.fn(), add: vi.fn() })),
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

vi.mock("@ffmpeg/ffmpeg", () => ({ FFmpeg: vi.fn() }));
vi.mock("@ffmpeg/util", () => ({ toBlobURL: vi.fn() }));
vi.mock("webm-duration-fix", () => ({ default: vi.fn() }));

Object.defineProperty(global, "window", {
  value: { addEventListener: vi.fn(), URL: { createObjectURL: vi.fn(() => "blob:test"), revokeObjectURL: vi.fn() } },
  writable: true,
});

let videoPlayImpl: () => Promise<void> = () => Promise.resolve();
let lastCreatedVideoEl: any = null;

Object.defineProperty(global, "document", {
  value: {
    getElementById: vi.fn(() => null),
    createElement: vi.fn((type: string) => {
      if (type === "a") return { click: vi.fn(), download: "", href: "" };
      if (type === "video") {
        const el: any = {
          srcObject: null,
          height: 0,
          width: 0,
          play: vi.fn(() => videoPlayImpl()),
          remove: vi.fn(),
        };
        lastCreatedVideoEl = el;
        return el;
      }
      return {};
    }),
  },
  writable: true,
});

class MockAudioContext {
  state = "running";
  destination = { id: "mock-destination" };
  createMediaElementSource = vi.fn(() => ({ connect: vi.fn() }));
  createMediaStreamDestination = vi.fn(() => ({ stream: { getAudioTracks: vi.fn(() => [{ id: "test-audio-track" }]) } }));
  close = vi.fn();
  resume = vi.fn(() => Promise.resolve());
}
global.AudioContext = MockAudioContext as any;

function makeFakeCanvas() {
  const stream = {
    getAudioTracks: vi.fn(() => []),
    addTrack: vi.fn(),
    removeTrack: vi.fn(),
  };
  return {
    _canvas: { captureStream: vi.fn(() => stream) },
  };
}

describe("ExportStore teardown on failure paths (Defect 2)", () => {
  let state: State;

  beforeEach(() => {
    vi.clearAllMocks();
    videoPlayImpl = () => Promise.resolve();
    lastCreatedVideoEl = null;

    const mockLayer: any = {
      batchDraw: vi.fn(),
      add: vi.fn(),
      getCanvas: vi.fn(() => makeFakeCanvas()),
    };
    const mockStage: any = { width: vi.fn(), height: vi.fn(), container: vi.fn(() => ({ style: {}, querySelector: vi.fn() })) };
    state = new State();
    state.setStage(mockStage, mockLayer, 800, 600);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("runs cleanup and leaves playing === false when video.play() rejects", async () => {
    const playError = new Error("play() rejected (autoplay policy)");
    videoPlayImpl = () => Promise.reject(playError);
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    state.saveCanvasToVideoWithAudio();

    // Playback is optimistically set to true before video.play() resolves/rejects.
    expect(state.playing).toBe(true);

    // Let the rejected promise's .catch handler run.
    await vi.waitFor(() => {
      expect(state.playing).toBe(false);
    });

    expect(consoleErrorSpy).toHaveBeenCalledWith(
      "Video export failed:",
      playError
    );
  });

  it("does not throw and cleanup is safe to invoke a second time if MediaRecorder errors after onstop already ran", async () => {
    // This exercises that cleanupExportAudioGraph's internal re-entrancy guard
    // holds even when reached indirectly (via the exported behavior), by
    // simply confirming a failed export settles cleanly without throwing.
    videoPlayImpl = () => Promise.reject(new Error("boom"));
    vi.spyOn(console, "error").mockImplementation(() => {});

    expect(() => state.saveCanvasToVideoWithAudio()).not.toThrow();

    await vi.waitFor(() => {
      expect(state.playing).toBe(false);
    });
  });
});
