import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import fixWebmDuration from "webm-duration-fix";
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
const audioElementsById = new Map<string, any>();

Object.defineProperty(global, "document", {
  value: {
    getElementById: vi.fn((id: string) => audioElementsById.get(id) ?? null),
    createElement: vi.fn((type: string) => {
      if (type === "a") {
        const anchor = { click: vi.fn(), download: "", href: "" };
        downloadAnchors.push(anchor);
        return anchor;
      }
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
  static instances: MockAudioContext[] = [];
  state = "running";
  destination = { id: "mock-destination" };
  createMediaElementSource = vi.fn(() => ({ connect: vi.fn() }));
  createMediaStreamDestination = vi.fn(() => ({ stream: { getAudioTracks: vi.fn(() => [{ id: "test-audio-track" }]) } }));
  createMediaStreamSource = vi.fn(() => ({ connect: vi.fn() }));
  close = vi.fn();
  resume = vi.fn(() => Promise.resolve());
  constructor() {
    MockAudioContext.instances.push(this);
  }
}
global.AudioContext = MockAudioContext as any;

// A MediaRecorder that lets tests drive the error/stop sequence the spec
// mandates: an error transitions the recorder to "inactive" and fires
// `error` followed by `stop`.
class MockMediaRecorder {
  static instances: MockMediaRecorder[] = [];
  state = "inactive";
  ondataavailable: ((e: any) => void) | null = null;
  onstop: (() => void) | null = null;
  onerror: ((e: any) => void) | null = null;
  start = vi.fn(() => {
    this.state = "recording";
  });
  stop = vi.fn(() => {
    this.state = "inactive";
    this.onstop?.();
  });
  constructor(public stream: any) {
    MockMediaRecorder.instances.push(this);
  }
  /** Drive the spec's error sequence: error event, then stop. */
  emitError(error: Error) {
    this.state = "inactive";
    this.onerror?.({ error });
    this.onstop?.();
  }
}
global.MediaRecorder = MockMediaRecorder as any;
global.URL = { createObjectURL: vi.fn(() => "blob:test"), revokeObjectURL: vi.fn() } as any;
global.Blob = class { constructor(public parts: any[], public opts: any) {} } as any;

const downloadAnchors: any[] = [];

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

function makeAudioEditorElement() {
  return {
    id: "a1",
    name: "Audio a1",
    type: "audio" as const,
    placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
    timeFrame: { start: 0, end: 5000 },
    properties: { elementId: "audio-el-1", src: "test.mp3", volume: 1, muted: false },
  } as any;
}

describe("ExportStore teardown on failure paths (Defect 2)", () => {
  let state: State;

  beforeEach(() => {
    vi.clearAllMocks();
    videoPlayImpl = () => Promise.resolve();
    lastCreatedVideoEl = null;
    MockAudioContext.instances.length = 0;
    MockMediaRecorder.instances.length = 0;
    downloadAnchors.length = 0;
    audioElementsById.clear();
    audioElementsById.set("audio-el-1", {
      tagName: "AUDIO",
      id: "audio-el-1",
      currentTime: 0,
      paused: true,
      play: vi.fn(),
      pause: vi.fn(),
    });

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

  it("closes the export mixer AudioContext exactly once when video.play() rejects", async () => {
    await state.addEditorElement(makeAudioEditorElement());
    videoPlayImpl = () => Promise.reject(new Error("play() rejected"));
    vi.spyOn(console, "error").mockImplementation(() => {});

    state.saveCanvasToVideoWithAudio();
    await vi.waitFor(() => expect(state.playing).toBe(false));

    // The mixer is the last AudioContext constructed (after each element's own).
    const mixer = MockAudioContext.instances.at(-1)!;
    expect(mixer.createMediaStreamSource).toHaveBeenCalled();
    expect(mixer.close).toHaveBeenCalledTimes(1);
  });

  it("surfaces a failure when post-processing rejects instead of failing silently", async () => {
    await state.addEditorElement(makeAudioEditorElement());
    videoPlayImpl = () => Promise.resolve();
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    // Stands in for the real silent-failure path: on the default mp4 format
    // onstop awaits ffmpeg.load(), which fetches its core over the network at
    // runtime. Nothing awaits the handler, so a rejection here used to vanish
    // and leave the UI looking like the export had succeeded.
    const postProcessingError = new Error("ffmpeg core unreachable");
    vi.mocked(fixWebmDuration).mockRejectedValue(postProcessingError);

    state.saveCanvasToVideoWithAudio();
    await vi.waitFor(() => expect(MockMediaRecorder.instances.length).toBe(1));

    MockMediaRecorder.instances[0].stop();

    await vi.waitFor(() => {
      expect(consoleErrorSpy).toHaveBeenCalledWith("Video export failed:", postProcessingError);
    });
    expect(state.playing).toBe(false);
    expect(downloadAnchors).toHaveLength(0);
  });

  it("tears down once and downloads nothing when the MediaRecorder errors mid-export", async () => {
    await state.addEditorElement(makeAudioEditorElement());
    videoPlayImpl = () => Promise.resolve();
    vi.spyOn(console, "error").mockImplementation(() => {});
    // Use webm so onstop's download path is reachable without the ffmpeg
    // branch, and give fixWebmDuration a real return value - otherwise onstop
    // throws before reaching the download and the assertion below proves
    // nothing.
    state.setVideoFormat("webm");
    vi.mocked(fixWebmDuration).mockResolvedValue({ size: 1 } as any);

    state.saveCanvasToVideoWithAudio();
    await vi.waitFor(() => expect(MockMediaRecorder.instances.length).toBe(1));

    const recorder = MockMediaRecorder.instances[0];
    const mixer = MockAudioContext.instances.at(-1)!;

    // Per spec an error fires `error` and then `stop`, so onstop runs too.
    recorder.emitError(new Error("recorder blew up"));
    await vi.waitFor(() => expect(state.playing).toBe(false));

    // Teardown happened exactly once despite both handlers running...
    expect(mixer.close).toHaveBeenCalledTimes(1);
    // ...and no partial file was handed to the user as a successful export.
    expect(downloadAnchors).toHaveLength(0);
  });
});
