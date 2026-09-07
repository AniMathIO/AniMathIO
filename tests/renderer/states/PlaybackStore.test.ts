import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { State } from "../../../renderer/states/state";

// Mock Konva before any imports
vi.mock("konva", () => {
  const mockLayer = {
    batchDraw: vi.fn(),
    add: vi.fn(),
    getCanvas: vi.fn(() => ({
      _canvas: {
        captureStream: vi.fn(() => ({
          getAudioTracks: vi.fn(() => []),
          addTrack: vi.fn(),
          removeTrack: vi.fn(),
        })),
      },
    })),
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

// Use the real isHtmlAudioElement/isHtmlVideoElement implementations (duck typing
// on tagName), since the test builds plain mock objects with tagName set.
vi.mock("../../../renderer/utils/konva-utils", () => ({
  KonvaAnimProxy: vi.fn().mockImplementation(() => ({})),
  KonvaUtils: { getClipRegion: vi.fn(() => ({ x: 0, y: 0, width: 100, height: 100 })) },
  getCoverCrop: vi.fn(() => ({ cropX: 0, cropY: 0, cropWidth: 100, cropHeight: 100 })),
  getFilterFromEffectType: vi.fn(() => "none"),
  makeImageSceneFunc: vi.fn(() => vi.fn()),
}));

Object.defineProperty(global, "window", {
  value: { addEventListener: vi.fn(), URL: { createObjectURL: vi.fn(() => "blob:test"), revokeObjectURL: vi.fn() } },
  writable: true,
});

// Registry of fake DOM audio elements keyed by id, so document.getElementById
// returns a stable, stateful mock we can assert against across calls.
const audioElementsById = new Map<string, any>();
// Same idea for fake <video> elements (updateVideoElements looks them up the
// same way); kept in its own map so tests can clear/assert them independently.
const videoElementsById = new Map<string, any>();

function makeFakeMediaElement(id: string, tagName: "AUDIO" | "VIDEO") {
  const el: any = {
    tagName,
    id,
    currentTime: 0,
    paused: true,
    play: vi.fn(() => {
      el.paused = false;
      return Promise.resolve();
    }),
    pause: vi.fn(() => {
      el.paused = true;
    }),
  };
  return el;
}

function makeFakeAudioElement(id: string) {
  return makeFakeMediaElement(id, "AUDIO");
}

function makeFakeVideoElement(id: string) {
  return makeFakeMediaElement(id, "VIDEO");
}

Object.defineProperty(global, "document", {
  value: {
    getElementById: vi.fn(
      (id: string) => audioElementsById.get(id) ?? videoElementsById.get(id) ?? null
    ),
    createElement: vi.fn((type: string) => {
      if (type === "a") return { click: vi.fn(), download: "", href: "" };
      if (type === "video") return { srcObject: null, height: 0, width: 0, play: vi.fn(() => Promise.resolve()), remove: vi.fn() };
      return {};
    }),
  },
  writable: true,
});

class MockAudioContext {
  destination = { id: "mock-destination" };
  createMediaElementSource = vi.fn(() => ({ connect: vi.fn() }));
  createMediaStreamDestination = vi.fn(() => ({ stream: { getAudioTracks: vi.fn(() => [{ id: "test-audio-track" }]) } }));
  close = vi.fn();
}
global.AudioContext = MockAudioContext as any;

function makeAudioElement(id: string, start: number, end: number) {
  return {
    id,
    name: `Audio ${id}`,
    type: "audio" as const,
    placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
    timeFrame: { start, end },
    properties: { elementId: `audio-el-${id}`, src: "test.mp3", volume: 1, muted: false },
  };
}

describe("PlaybackStore audio sync (Bug 1)", () => {
  let state: State;

  beforeEach(async () => {
    vi.clearAllMocks();
    audioElementsById.clear();

    const mockLayer: any = {
      batchDraw: vi.fn(),
      add: vi.fn(),
      getCanvas: vi.fn(() => ({ getElement: vi.fn(() => ({ captureStream: vi.fn() })) })),
    };
    const mockStage: any = {
      width: vi.fn(),
      height: vi.fn(),
      container: vi.fn(() => ({ style: { backgroundColor: "" }, querySelector: vi.fn() })),
    };

    state = new State();
    state.setStage(mockStage, mockLayer, 800, 600);

    // Clip A at [0s, 4s], clip B at [5s, 9s]
    audioElementsById.set("audio-el-A", makeFakeAudioElement("audio-el-A"));
    audioElementsById.set("audio-el-B", makeFakeAudioElement("audio-el-B"));
    await state.addEditorElement(makeAudioElement("A", 0, 4000) as any);
    await state.addEditorElement(makeAudioElement("B", 5000, 9000) as any);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("plays clip A and keeps clip B paused/reset at t=1s", () => {
    state.playback.currentKeyFrame = Math.floor((1000 / 1000) * state.fps);
    (state.playback as any).playing = true;
    state.updateAudioElements();

    const a = audioElementsById.get("audio-el-A");
    const b = audioElementsById.get("audio-el-B");

    expect(a.play).toHaveBeenCalled();
    expect(a.paused).toBe(false);
    expect(Math.abs(a.currentTime - 1)).toBeLessThan(0.2);

    expect(b.pause).toHaveBeenCalled();
    expect(b.currentTime).toBe(0);
  });

  it("plays clip B (currentTime~=1s) and pauses/resets clip A at t=6s", () => {
    state.playback.currentKeyFrame = Math.floor((6000 / 1000) * state.fps);
    (state.playback as any).playing = true;
    state.updateAudioElements();

    const a = audioElementsById.get("audio-el-A");
    const b = audioElementsById.get("audio-el-B");

    expect(b.play).toHaveBeenCalled();
    expect(b.paused).toBe(false);
    expect(Math.abs(b.currentTime - 1)).toBeLessThan(0.2);

    expect(a.pause).toHaveBeenCalled();
    expect(a.currentTime).toBe(0);
  });

  it("does not reassign currentTime when already close enough (avoids stutter)", () => {
    const a = audioElementsById.get("audio-el-A");
    a.currentTime = 1.0;

    state.playback.currentKeyFrame = Math.floor((1050 / 1000) * state.fps); // 1.05s
    (state.playback as any).playing = true;
    state.updateAudioElements();

    // Within the 0.15s tolerance, so currentTime must be left untouched.
    expect(a.currentTime).toBe(1.0);
  });

  it("updateTimeTo re-syncs audio during ongoing playback (throttled)", () => {
    (state.playback as any).playing = true;
    const spy = vi.spyOn(state.playback, "updateAudioElements");

    state.updateTimeTo(100); // small delta from 0 -> below 250ms throttle
    expect(spy).not.toHaveBeenCalled();

    state.updateTimeTo(400); // >= 250ms since last sync -> should resync
    expect(spy).toHaveBeenCalled();
  });
});

function makeVideoElement(id: string, start: number, end: number) {
  return {
    id,
    name: `Video ${id}`,
    type: "video" as const,
    placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
    timeFrame: { start, end },
    properties: {
      elementId: `video-el-${id}`,
      src: "test.mp4",
      effect: { type: "none" as const },
      muted: false,
    },
  };
}

function makeTestState() {
  const mockLayer: any = {
    batchDraw: vi.fn(),
    add: vi.fn(),
    getCanvas: vi.fn(() => ({ getElement: vi.fn(() => ({ captureStream: vi.fn() })) })),
  };
  const mockStage: any = {
    width: vi.fn(),
    height: vi.fn(),
    container: vi.fn(() => ({ style: { backgroundColor: "" }, querySelector: vi.fn() })),
  };
  const state = new State();
  state.setStage(mockStage, mockLayer, 800, 600);
  return state;
}

describe("PlaybackStore video sync during playback (Defect 1)", () => {
  let state: State;

  beforeEach(async () => {
    vi.clearAllMocks();
    audioElementsById.clear();
    videoElementsById.clear();

    state = makeTestState();

    // One video clip at [5s, 10s].
    videoElementsById.set("video-el-V", makeFakeVideoElement("video-el-V"));
    await state.addEditorElement(makeVideoElement("V", 5000, 10000) as any);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  /**
   * Drives the playback loop's own entry point (updateTimeTo) rather than
   * calling updateVideoElements() directly, because the regression is that
   * updateTimeTo never re-evaluated video elements at all.
   */
  function playThrough(state: State, times: number[]) {
    state.setPlaying(true);
    for (const t of times) state.updateTimeTo(t);
  }

  it("starts the clip when playback reaches its timeFrame", () => {
    playThrough(state, [1000, 2000, 3000, 4000, 4900, 5000, 5500, 6000]);

    const v = videoElementsById.get("video-el-V");
    expect(v.play).toHaveBeenCalled();
    expect(v.paused).toBe(false);
    expect(Math.abs(v.currentTime - 1)).toBeLessThan(0.2);
  });

  it("keeps the clip paused and reset before its timeFrame", () => {
    playThrough(state, [1000, 2000]);

    const v = videoElementsById.get("video-el-V");
    expect(v.paused).toBe(true);
    expect(v.currentTime).toBe(0);
  });

  it("pauses and resets the clip after its timeFrame", () => {
    playThrough(state, [5000, 6000, 9000, 10000, 11000, 12000]);

    const v = videoElementsById.get("video-el-V");
    expect(v.paused).toBe(true);
    expect(v.currentTime).toBe(0);
  });

  afterEach(() => {
    // Stop the requestAnimationFrame loop setPlaying(true) kicked off, so it
    // can't tick against the next test's cleared element registry.
    state.setPlaying(false);
  });
});

describe("PlaybackStore boundary-aware sync throttle (Defect 2)", () => {
  let state: State;

  beforeEach(async () => {
    vi.clearAllMocks();
    audioElementsById.clear();
    videoElementsById.clear();

    state = makeTestState();

    // Audio clip at [5s, 9s], video clip at [12s, 14s].
    audioElementsById.set("audio-el-A", makeFakeAudioElement("audio-el-A"));
    videoElementsById.set("video-el-V", makeFakeVideoElement("video-el-V"));
    await state.addEditorElement(makeAudioElement("A", 5000, 9000) as any);
    await state.addEditorElement(makeVideoElement("V", 12000, 14000) as any);
  });

  afterEach(() => {
    // Stop the requestAnimationFrame loop setPlaying(true) kicked off before
    // the spies are torn down.
    state.setPlaying(false);
    vi.restoreAllMocks();
  });

  it("syncs immediately when a tick crosses an audio clip boundary", () => {
    state.setPlaying(true);
    state.updateTimeTo(4900);

    const audioSpy = vi.spyOn(state.playback, "updateAudioElements");
    const videoSpy = vi.spyOn(state.playback, "updateVideoElements");

    // Only ~110ms later — well inside the 250ms drift throttle — but it steps
    // over the clip's 5s start, so it must sync anyway.
    state.updateTimeTo(5010);

    expect(audioSpy).toHaveBeenCalled();
    expect(videoSpy).toHaveBeenCalled();
  });

  it("syncs immediately when a tick crosses a video clip boundary", () => {
    state.setPlaying(true);
    state.updateTimeTo(11900);

    const videoSpy = vi.spyOn(state.playback, "updateVideoElements");
    state.updateTimeTo(12010);

    expect(videoSpy).toHaveBeenCalled();
  });

  it("stays throttled on a tick that crosses no boundary", () => {
    state.setPlaying(true);
    state.updateTimeTo(1000);

    const audioSpy = vi.spyOn(state.playback, "updateAudioElements");
    const videoSpy = vi.spyOn(state.playback, "updateVideoElements");

    state.updateTimeTo(1100); // 100ms later, no boundary in between
    expect(audioSpy).not.toHaveBeenCalled();
    expect(videoSpy).not.toHaveBeenCalled();

    state.updateTimeTo(1400); // 400ms since last sync -> drift correction fires
    expect(audioSpy).toHaveBeenCalled();
    expect(videoSpy).toHaveBeenCalled();
  });

  it("detects a boundary crossed while jumping backwards", () => {
    state.setPlaying(true);
    state.updateTimeTo(5100);

    const audioSpy = vi.spyOn(state.playback, "updateAudioElements");
    state.updateTimeTo(4990); // backwards over the 5s start

    expect(audioSpy).toHaveBeenCalled();
  });

  it("stops the clip at its end without waiting out the throttle", () => {
    state.setPlaying(true);
    state.updateTimeTo(8900);

    const a = audioElementsById.get("audio-el-A");
    expect(a.paused).toBe(false);

    state.updateTimeTo(9020); // 120ms later, just past the 9s end
    expect(a.paused).toBe(true);
    expect(a.currentTime).toBe(0);
  });
});

describe("updateEditorElementTimeFrame media sync (Defect 3)", () => {
  let state: State;

  beforeEach(async () => {
    vi.clearAllMocks();
    audioElementsById.clear();
    videoElementsById.clear();
    state = makeTestState();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("evaluates media against the NEW timeframe when moved while paused", async () => {
    audioElementsById.set("audio-el-M", makeFakeAudioElement("audio-el-M"));
    const element = makeAudioElement("M", 0, 4000);
    await state.addEditorElement(element as any);

    const a = audioElementsById.get("audio-el-M");
    // Paused at t=1s, inside the original [0s, 4s] frame.
    state.playback.currentKeyFrame = Math.floor((1000 / 1000) * state.fps);
    a.currentTime = 1;
    a.paused = false;

    // Drag the clip to [6s, 10s]: t=1s is now outside it.
    await state.updateEditorElementTimeFrame(state.editorElements[0] as any, {
      start: 6000,
      end: 10000,
    });

    expect(a.paused).toBe(true);
    expect(a.currentTime).toBe(0);
  });

  it("re-syncs a clip that the new timeframe brings under the playhead", async () => {
    audioElementsById.set("audio-el-N", makeFakeAudioElement("audio-el-N"));
    await state.addEditorElement(makeAudioElement("N", 6000, 10000) as any);

    const a = audioElementsById.get("audio-el-N");
    state.playback.currentKeyFrame = Math.floor((7000 / 1000) * state.fps);

    // Resize the clip to start at 5s; the playhead at 7s is 2s into it.
    await state.updateEditorElementTimeFrame(state.editorElements[0] as any, { start: 5000 });

    expect(Math.abs(a.currentTime - 2)).toBeLessThan(0.2);
  });
});

describe("RootStore.getAudioContext (Bug 2)", () => {
  it("connects the source node to ctx.destination so preview audio survives an export", () => {
    const state = new State();
    const fakeAudioEl = { id: "audio-preview-1" } as any;

    const entry = state.getAudioContext(fakeAudioEl);

    expect(entry.sourceNode.connect).toHaveBeenCalledWith(entry.context.destination);
  });

  it("reuses the cached context/source node on subsequent calls", () => {
    const state = new State();
    const fakeAudioEl = { id: "audio-preview-2" } as any;

    const first = state.getAudioContext(fakeAudioEl);
    const second = state.getAudioContext(fakeAudioEl);

    expect(second).toBe(first);
  });
});

describe("Audio context cleanup on element removal (Bug 3)", () => {
  let state: State;

  beforeEach(() => {
    vi.clearAllMocks();
    audioElementsById.clear();
    const mockLayer: any = { batchDraw: vi.fn(), add: vi.fn(), getCanvas: vi.fn(() => ({})) };
    const mockStage: any = { width: vi.fn(), height: vi.fn(), container: vi.fn(() => ({ style: {}, querySelector: vi.fn() })) };
    state = new State();
    state.setStage(mockStage, mockLayer, 800, 600);
  });

  it("removes and closes the AudioContext entry when its audio element is deleted", async () => {
    const el = makeAudioElement("del", 0, 1000);
    await state.addEditorElement(el as any);

    const fakeAudioEl = { id: el.properties.elementId } as any;
    const entry = state.getAudioContext(fakeAudioEl);
    expect(state.audioContexts.has(el.properties.elementId)).toBe(true);

    state.removeEditorElement("del");

    expect(state.audioContexts.has(el.properties.elementId)).toBe(false);
    expect(entry.context.close).toHaveBeenCalled();
  });

  it("does not leave a growing audioContexts map across repeated get/release cycles", () => {
    for (let i = 0; i < 5; i++) {
      const fakeAudioEl = { id: `audio-cycle-${i}` } as any;
      state.getAudioContext(fakeAudioEl);
      state.releaseAudioContext(`audio-cycle-${i}`);
    }
    expect(state.audioContexts.size).toBe(0);
  });
});
