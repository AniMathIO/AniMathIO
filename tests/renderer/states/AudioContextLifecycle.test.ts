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

Object.defineProperty(global, "window", {
  value: { addEventListener: vi.fn(), URL: { createObjectURL: vi.fn(() => "blob:test"), revokeObjectURL: vi.fn() } },
  writable: true,
});

// Element ids that should look present in the DOM. A real <audio> node
// survives a project re-open (React reconciles it by id), which is exactly the
// case where its cached AudioContext must NOT be closed.
const mountedElementIds = new Set<string>();

Object.defineProperty(global, "document", {
  value: {
    getElementById: vi.fn((id: string) => (mountedElementIds.has(id) ? { id, tagName: "AUDIO" } : null)),
    createElement: vi.fn((type: string) => {
      if (type === "a") return { click: vi.fn(), download: "", href: "" };
      if (type === "video") return { srcObject: null, height: 0, width: 0, play: vi.fn(() => Promise.resolve()), remove: vi.fn() };
      return {};
    }),
  },
  writable: true,
});

// A MockAudioContext that tracks its own open/closed/suspended state so
// tests can assert on it, mirroring the real AudioContext state machine.
class MockAudioContext {
  state: "running" | "suspended" | "closed" = "running";
  destination = { id: "mock-destination" };
  createMediaElementSource = vi.fn(() => ({ connect: vi.fn() }));
  createMediaStreamDestination = vi.fn(() => ({ stream: { getAudioTracks: vi.fn(() => [{ id: "test-audio-track" }]) } }));
  close = vi.fn(() => {
    this.state = "closed";
    return Promise.resolve();
  });
  resume = vi.fn(() => {
    this.state = "running";
    return Promise.resolve();
  });
}
let lastCreatedContexts: MockAudioContext[] = [];
global.AudioContext = vi.fn(function (this: any) {
  const ctx = new MockAudioContext();
  lastCreatedContexts.push(ctx);
  return ctx;
}) as any;

function makeAudioElement(id: string, elementId: string) {
  return {
    id,
    name: `Audio ${id}`,
    type: "audio" as const,
    placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
    timeFrame: { start: 0, end: 1000 },
    properties: { elementId, src: "test.mp3", volume: 1, muted: false },
  };
}

describe("AudioContext retention for elements that survive a project switch", () => {
  let state: State;

  beforeEach(() => {
    vi.clearAllMocks();
    lastCreatedContexts = [];
    mountedElementIds.clear();
    state = new State();
  });

  afterEach(() => {
    mountedElementIds.clear();
    vi.restoreAllMocks();
  });

  it("keeps the context of an element whose <audio> node is still mounted", () => {
    mountedElementIds.add("audio-el-keep");
    const el = { id: "audio-el-keep" } as HTMLAudioElement;

    const entry = state.getAudioContext(el);
    expect(lastCreatedContexts).toHaveLength(1);

    state.releaseDetachedAudioContexts();

    // A MediaElementAudioSourceNode binding is permanent for the element's
    // lifetime: closing this context would leave the element permanently
    // unusable, and the next createMediaElementSource would throw
    // InvalidStateError, killing the next export before it starts.
    expect(entry.context.state).not.toBe("closed");
    expect(state.audioContexts.has("audio-el-keep")).toBe(true);

    // Re-requesting must hand back the same context, never build a second one.
    expect(state.getAudioContext(el)).toBe(entry);
    expect(lastCreatedContexts).toHaveLength(1);
  });

  it("still closes contexts whose element has left the document", () => {
    mountedElementIds.add("audio-el-gone");
    const el = { id: "audio-el-gone" } as HTMLAudioElement;
    const entry = state.getAudioContext(el);

    mountedElementIds.delete("audio-el-gone");
    state.releaseDetachedAudioContexts();

    expect(entry.context.state).toBe("closed");
    expect(state.audioContexts.has("audio-el-gone")).toBe(false);
  });
});

describe("AudioContext lifecycle across project switch (Defect 1)", () => {
  let state: State;

  beforeEach(() => {
    vi.clearAllMocks();
    lastCreatedContexts = [];

    const mockLayer: any = { batchDraw: vi.fn(), add: vi.fn(), getCanvas: vi.fn(() => ({})) };
    const mockStage: any = { width: vi.fn(), height: vi.fn(), container: vi.fn(() => ({ style: {}, querySelector: vi.fn() })) };
    state = new State();
    state.setStage(mockStage, mockLayer, 800, 600);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("closes every cached context and empties the map after resetForNewProject()", async () => {
    const elA = makeAudioElement("a", "audio-el-a");
    const elB = makeAudioElement("b", "audio-el-b");
    await state.addEditorElement(elA as any);
    await state.addEditorElement(elB as any);

    const entryA = state.getAudioContext({ id: "audio-el-a" } as any);
    const entryB = state.getAudioContext({ id: "audio-el-b" } as any);
    expect(state.audioContexts.size).toBe(2);

    state.resetForNewProject();

    expect(state.audioContexts.size).toBe(0);
    expect(entryA.context.close).toHaveBeenCalled();
    expect(entryB.context.close).toHaveBeenCalled();
  });

  it("closes every cached context and empties the map before deserialize() assigns new elements", () => {
    const entry = state.getAudioContext({ id: "audio-el-old" } as any);
    expect(state.audioContexts.size).toBe(1);

    const stateObject = {
      backgroundColor: "#111111",
      selectedMenuOption: "Videos",
      audios: [],
      videos: [],
      images: [],
      editorElements: [],
      maxTime: 30000,
      animations: [],
      currentKeyFrame: 0,
      fps: 60,
      selectedVideoFormat: "mp4",
      canvas_width: 800,
      canvas_height: 600,
      mediaFiles: [],
    };
    const json = JSON.stringify(stateObject);
    const buffer = new TextEncoder().encode(json).buffer;

    state.deserialize(buffer as ArrayBuffer);

    expect(state.audioContexts.size).toBe(0);
    expect(entry.context.close).toHaveBeenCalled();
  });

  it("keeps the cached-context count flat across a simulated project switch (no leak across N projects)", () => {
    for (let project = 0; project < 5; project++) {
      state.getAudioContext({ id: `audio-project-${project}-clip-1` } as any);
      state.getAudioContext({ id: `audio-project-${project}-clip-2` } as any);
      expect(state.audioContexts.size).toBe(2);
      state.resetForNewProject();
      expect(state.audioContexts.size).toBe(0);
    }

    // Every context created across all 5 "projects" was actually closed —
    // none were left open, which is the leak this test guards against.
    expect(lastCreatedContexts).toHaveLength(10);
    lastCreatedContexts.forEach((ctx) => expect(ctx.close).toHaveBeenCalled());
  });

  it("resumes a suspended context (e.g. created outside a user gesture) when handed out", () => {
    const audioEl = { id: "audio-suspended" } as any;
    const entry = state.getAudioContext(audioEl);
    (entry.context as any).state = "suspended";

    // Fetching the same element's context again should resume it.
    const again = state.getAudioContext(audioEl);

    expect(again).toBe(entry);
    expect(entry.context.resume).toHaveBeenCalled();
  });
});
