import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import fixWebmDuration from "webm-duration-fix";
import { FFmpeg } from "@ffmpeg/ffmpeg";
import { toBlobURL } from "@ffmpeg/util";
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
global.Blob = class {
  constructor(public parts: any[], public opts: any) {}
  async arrayBuffer() {
    return new Uint8Array(this.parts.flatMap(part => Array.from(part))).buffer;
  }
} as any;

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

// Drive the real recorder completion and IPC delivery paths. A spy on the
// export method alone would miss early resolution and partial-file delivery.
describe("ExportStore destination delivery", () => {
  let state: State;
  let writeVideoFile: ReturnType<typeof vi.fn>;
  const outputBytes = new Uint8Array([0, 127, 128, 255]);

  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    videoPlayImpl = () => Promise.resolve();
    MockMediaRecorder.instances.length = 0;
    downloadAnchors.length = 0;
    audioElementsById.clear();
    writeVideoFile = vi.fn().mockResolvedValue({ success: true });
    (window as any).electron = { writeVideoFile };
    state = new State();
    state.setStage({ width: vi.fn(), height: vi.fn(), container: vi.fn(() => ({ style: {}, querySelector: vi.fn() })) } as any,
      { batchDraw: vi.fn(), add: vi.fn(), getCanvas: vi.fn(() => makeFakeCanvas()) } as any, 800, 600);
    state.setMaxTime(1000);
    vi.mocked(fixWebmDuration).mockResolvedValue({ arrayBuffer: async () => outputBytes.buffer } as any);
    vi.spyOn(console, "error").mockImplementation(() => {});
  });
  afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

  it("records for the project duration, delivers exact WebM bytes, and waits for the disk write", async () => {
    let finishWrite!: (result: { success: boolean }) => void;
    writeVideoFile.mockImplementation(() => new Promise(resolve => { finishWrite = resolve; }));
    let settled = false;
    const pending = state.exportStore.saveCanvasToVideoToPath("/tmp/result.webm", "webm").then(() => { settled = true; });
    await vi.advanceTimersByTimeAsync(0);
    expect(MockMediaRecorder.instances[0].start).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(999);
    expect(writeVideoFile).not.toHaveBeenCalled();
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(MockMediaRecorder.instances[0].stop).toHaveBeenCalledOnce();
    expect(writeVideoFile).toHaveBeenCalledExactlyOnceWith("/tmp/result.webm", [0, 127, 128, 255]);
    expect(settled).toBe(false);
    finishWrite({ success: true });
    await pending;
    expect(settled).toBe(true);
    expect(state.playing).toBe(false);
    expect(downloadAnchors).toHaveLength(0);
  });

  it("rejects disk-write failures and allows a subsequent export", async () => {
    writeVideoFile.mockResolvedValueOnce({ success: false, error: "EACCES destination" });
    const pending = state.exportStore.saveCanvasToVideoToPath("/tmp/result.webm", "webm");
    const failure = expect(pending).rejects.toThrow("EACCES destination");
    await vi.advanceTimersByTimeAsync(1000);
    await failure;
    expect(downloadAnchors).toHaveLength(0);
    const retry = state.exportStore.saveCanvasToVideoToPath("/tmp/retry.webm", "webm");
    await vi.advanceTimersByTimeAsync(1000);
    await retry;
    expect(writeVideoFile).toHaveBeenLastCalledWith("/tmp/retry.webm", [0, 127, 128, 255]);
  });

  it("rejects recorder errors, cancels the stop timer, and never writes partial bytes", async () => {
    const pending = state.exportStore.saveCanvasToVideoToPath("/tmp/result.webm", "webm");
    const failure = expect(pending).rejects.toThrow("recorder failed");
    await vi.advanceTimersByTimeAsync(0);
    const recorder = MockMediaRecorder.instances[0];
    recorder.emitError(new Error("recorder failed"));
    await failure;
    await vi.advanceTimersByTimeAsync(5000);
    expect(recorder.stop).not.toHaveBeenCalled();
    expect(fixWebmDuration).not.toHaveBeenCalled();
    expect(writeVideoFile).not.toHaveBeenCalled();
    expect(downloadAnchors).toHaveLength(0);
    expect(state.playing).toBe(false);
  });

  it("rejects failed playback without writing and blocks overlapping exports", async () => {
    let failPlay!: (error: Error) => void;
    videoPlayImpl = () => new Promise((_resolve, reject) => { failPlay = reject; });
    const pending = state.exportStore.saveCanvasToVideoToPath("/tmp/first.webm", "webm");
    const failure = expect(pending).rejects.toThrow("play failed");
    await expect(state.exportStore.saveCanvasToVideoToPath("/tmp/second.webm", "webm")).rejects.toThrow("Another video export");
    failPlay(new Error("play failed"));
    await failure;
    expect(writeVideoFile).not.toHaveBeenCalled();
    expect(state.playing).toBe(false);
  });

  it("rejects conversion failures without writing or downloading a partial file", async () => {
    vi.mocked(fixWebmDuration).mockRejectedValue(new Error("conversion failed"));
    const pending = state.exportStore.saveCanvasToVideoToPath("/tmp/result.mp4", "mp4");
    const failure = expect(pending).rejects.toThrow("conversion failed");
    await vi.advanceTimersByTimeAsync(1000);
    await failure;
    expect(writeVideoFile).not.toHaveBeenCalled();
    expect(downloadAnchors).toHaveLength(0);
  });

  it("writes converted MP4 bytes instead of the source WebM and keeps the UI format", async () => {
    const converted = new Uint8Array([9, 8, 7]);
    const ffmpeg = { load: vi.fn().mockResolvedValue(undefined), writeFile: vi.fn().mockResolvedValue(undefined),
      exec: vi.fn().mockResolvedValue(0), readFile: vi.fn().mockResolvedValue(converted) };
    vi.mocked(FFmpeg).mockImplementation(function () { return ffmpeg as any; });
    vi.mocked(toBlobURL).mockResolvedValue("blob:ffmpeg-core");
    state.setVideoFormat("webm");
    const pending = state.exportStore.saveCanvasToVideoToPath("/tmp/result.mp4", "mp4");
    await vi.advanceTimersByTimeAsync(1000);
    await pending;
    expect(ffmpeg.load).toHaveBeenCalledOnce();
    expect(ffmpeg.writeFile).toHaveBeenCalledWith("video.webm", outputBytes);
    expect(ffmpeg.exec).toHaveBeenCalledWith(expect.arrayContaining(["libx264", "video.mp4"]));
    expect(ffmpeg.readFile).toHaveBeenCalledWith("video.mp4");
    expect(writeVideoFile).toHaveBeenCalledExactlyOnceWith("/tmp/result.mp4", [9, 8, 7]);
    expect(state.selectedVideoFormat).toBe("webm");
    expect(downloadAnchors).toHaveLength(0);
  });

  it("rejects a nonzero FFmpeg exit code even if an output file could be read", async () => {
    const ffmpeg = { load: vi.fn().mockResolvedValue(undefined), writeFile: vi.fn().mockResolvedValue(undefined),
      exec: vi.fn().mockResolvedValue(1), readFile: vi.fn().mockResolvedValue(new Uint8Array([99])) };
    vi.mocked(FFmpeg).mockImplementation(function () { return ffmpeg as any; });
    vi.mocked(toBlobURL).mockResolvedValue("blob:ffmpeg-core");
    const pending = state.exportStore.saveCanvasToVideoToPath("/tmp/result.mp4", "mp4");
    const failure = expect(pending).rejects.toThrow("FFmpeg exit code 1");
    await vi.advanceTimersByTimeAsync(1000);
    await failure;
    expect(ffmpeg.readFile).not.toHaveBeenCalled();
    expect(writeVideoFile).not.toHaveBeenCalled();
    expect(downloadAnchors).toHaveLength(0);
  });

  it("rejects an FFmpeg load failure without writing", async () => {
    vi.mocked(FFmpeg).mockImplementation(function () { return { load: vi.fn().mockRejectedValue(new Error("FFmpeg unavailable")) } as any; });
    vi.mocked(toBlobURL).mockResolvedValue("blob:ffmpeg-core");
    const pending = state.exportStore.saveCanvasToVideoToPath("/tmp/result.mp4", "mp4");
    const failure = expect(pending).rejects.toThrow("FFmpeg unavailable");
    await vi.advanceTimersByTimeAsync(1000);
    await failure;
    expect(writeVideoFile).not.toHaveBeenCalled();
    expect(downloadAnchors).toHaveLength(0);
  });

  it("does not deliver a file if recording fails while post-processing is awaiting", async () => {
    let completeFix!: (blob: any) => void;
    vi.mocked(fixWebmDuration).mockImplementation(() => new Promise(resolve => { completeFix = resolve; }));
    const pending = state.exportStore.saveCanvasToVideoToPath("/tmp/result.webm", "webm");
    const failure = expect(pending).rejects.toThrow("late recorder error");
    await vi.advanceTimersByTimeAsync(1000);
    MockMediaRecorder.instances[0].emitError(new Error("late recorder error"));
    await failure;
    completeFix({ arrayBuffer: async () => outputBytes.buffer });
    await vi.advanceTimersByTimeAsync(0);
    expect(writeVideoFile).not.toHaveBeenCalled();
    expect(downloadAnchors).toHaveLength(0);
  });

  it("rejects when no canvas or capture stream is available", async () => {
    state.canvasStore.layer = null;
    await expect(state.exportStore.saveCanvasToVideoToPath("/tmp/result.webm", "webm")).rejects.toThrow("No Konva layer");
    expect(writeVideoFile).not.toHaveBeenCalled();
  });

  it("retains the UI WebM download route", async () => {
    state.setVideoFormat("webm");
    state.saveCanvasToVideoWithAudio();
    await vi.advanceTimersByTimeAsync(1000);
    expect(downloadAnchors).toHaveLength(1);
    expect(downloadAnchors[0].download).toBe("video.webm");
    expect(downloadAnchors[0].click).toHaveBeenCalledOnce();
    expect(writeVideoFile).not.toHaveBeenCalled();
  });
});
