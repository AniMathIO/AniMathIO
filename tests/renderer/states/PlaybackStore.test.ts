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

function makeFakeAudioElement(id: string) {
  const el: any = {
    tagName: "AUDIO",
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

Object.defineProperty(global, "document", {
  value: {
    getElementById: vi.fn((id: string) => audioElementsById.get(id) ?? null),
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
