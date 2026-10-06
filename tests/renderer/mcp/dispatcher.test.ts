import { beforeEach, describe, expect, it, vi } from "vitest";
import { State } from "../../../renderer/states/state";
import { createMcpDispatcher } from "../../../renderer/mcp/dispatcher";
import { renderLatexToImage } from "../../../renderer/utils/katex-render";
import { MCP_TOOLS } from "../../../main/mcp/tool-schemas";

vi.mock("konva", () => ({ default: {} }));
vi.mock("animejs", () => ({
  createTimeline: vi.fn(() => ({ add: vi.fn(), seek: vi.fn(), revert: vi.fn() })),
}));
vi.mock("../../../renderer/utils/katex-render", () => ({
  renderLatexToImage: vi.fn(),
}));

describe("MCP dispatcher", () => {
  let state: State;
  let dispatch: ReturnType<typeof createMcpDispatcher>;

  beforeEach(() => {
    vi.clearAllMocks();
    state = new State();
    dispatch = createMcpDispatcher(state);
  });

  it.each(MCP_TOOLS.filter(({ name }) => name !== "get_project_state").map(({ name }) => name))(
    "%s requires an open project before validating arguments",
    async (tool) => {
      expect(state.isEditorActive).toBe(false);
      await expect(dispatch(tool, {})).rejects.toThrow(
        "No project is open in AniMathIO. Create or open one before using this tool."
      );
      expect(state.editorElements).toHaveLength(0);
      expect(state.animations).toHaveLength(0);
    }
  );

  it.each([false, true])("reports project state when editor active is %s", async (active) => {
    state.setEditorActive(active);
    state.setCanvasSize(960, 540);
    state.setBackgroundColor("#123456");
    state.setMaxTime(12000);
    state.addText({ text: "Existing text", fontSize: 24, fontWeight: 400 });
    const element = state.editorElements[0];

    await expect(dispatch("get_project_state", {})).resolves.toEqual({
      isEditorActive: active,
      canvas: { width: 960, height: 540, backgroundColor: "#123456" },
      maxTimeMs: 12000,
      currentTimeMs: 0,
      elements: [{
        id: element.id,
        name: "Text 1",
        type: "text",
        placement: { x: 0, y: 0, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
        timeFrame: { start: 0, end: 12000 },
      }],
      animations: [],
    });
  });

  it("creates text and returns the new element's id", async () => {
    state.setEditorActive(true);
    state.addText({ text: "Already here", fontSize: 12, fontWeight: 400 });
    const result = await dispatch("add_text", { text: "Hello MCP", fontSize: 48, fontWeight: 700 });

    expect(state.editorElements).toHaveLength(2);
    const created = state.editorElements[1];
    expect(result).toEqual({ id: created.id });
    expect(created.id).toEqual(expect.any(String));
    expect(created.id).not.toBe(state.editorElements[0].id);
    expect(created).toMatchObject({
      type: "text",
      properties: { text: "Hello MCP", fontSize: 48, fontWeight: 700 },
      timeFrame: { start: 0, end: state.maxTime },
    });
  });

  it("returns a detached, structured-cloneable snapshot of observable elements", async () => {
    state.setEditorActive(true);
    state.addText({ text: "IPC snapshot", fontSize: 24, fontWeight: 400 });
    const result = await dispatch("get_project_state", {});
    const cloned = structuredClone(result) as { elements: { placement: { x: number }; timeFrame: { end: number } }[] };
    expect(cloned).toEqual(result);
    cloned.elements[0].placement.x = 99;
    cloned.elements[0].timeFrame.end = 100;
    expect(state.editorElements[0].placement.x).toBe(0);
    expect(state.editorElements[0].timeFrame.end).toBe(state.maxTime);
  });

  it.each(["update_element", "remove_element"])("%s identifies an unknown id", async (tool) => {
    state.setEditorActive(true);
    await expect(dispatch(tool, { id: "missing-element-42" })).rejects.toThrow(
      'No element with id "missing-element-42"'
    );
  });

  it("rejects an unknown animation type and lists all valid types", async () => {
    state.setEditorActive(true);
    const error = dispatch("add_animation", { targetId: "missing", type: "spin", durationMs: 500 });
    await expect(error).rejects.toThrow(
      '"spin" is not a known animation. Use one of: fadeIn, fadeOut, slideIn, slideOut, breathe, mafsReveal.'
    );
    expect(state.animations).toHaveLength(0);
  });

  it("rejects an unknown tool by name", async () => {
    await expect(dispatch("does_not_exist", {})).rejects.toThrow('Unknown tool "does_not_exist".');
  });

  it("updates an element while preserving unspecified placement and time fields, then removes it", async () => {
    state.setEditorActive(true);
    await dispatch("add_text", { text: "Editable" });
    const { id, placement, timeFrame } = state.editorElements[0];
    const originalPlacement = { ...placement };
    const originalTimeFrame = { ...timeFrame };

    await expect(dispatch("update_element", { id, placement: { x: 123 }, timeFrame: { start: 500 } }))
      .resolves.toEqual({ id });
    expect(state.editorElements[0]).toMatchObject({
      placement: { ...originalPlacement, x: 123 },
      timeFrame: { ...originalTimeFrame, start: 500 },
      properties: { text: "Editable" },
    });
    await expect(dispatch("remove_element", { id })).resolves.toEqual({ removed: true });
    expect(state.editorElements).toHaveLength(0);
  });

  it("adds rasterised math with canvas-centred placement and the project duration", async () => {
    state.setEditorActive(true);
    state.setCanvasSize(800, 600);
    state.setMaxTime(9000);
    vi.mocked(renderLatexToImage).mockResolvedValue({ dataUrl: "data:image/png;base64,test", width: 200, height: 80 });

    const result = await dispatch("add_math", { latex: "x^2", color: "#ffffff" });
    expect(state.editorElements).toHaveLength(1);
    const element = state.editorElements[0];
    expect(result).toEqual({ id: element.id });
    expect(renderLatexToImage).toHaveBeenCalledWith("x^2", { color: "#ffffff" });
    expect(element).toMatchObject({
      type: "mafs",
      placement: { x: 300, y: 260, width: 200, height: 80 },
      timeFrame: { start: 0, end: 9000 },
      properties: { src: "data:image/png;base64,test" },
    });
  });
});

// These tests use the real stores so a wrong resource index or premature add
// produces missing/invalid timeline elements, rather than passing on a spy.
vi.mock("../../../renderer/utils/domLoad", () => ({
  waitForElementById: vi.fn(),
  waitForMediaReady: vi.fn(),
}));
import { waitForElementById, waitForMediaReady } from "../../../renderer/utils/domLoad";

describe("MCP media and export", () => {
  let state: State;
  let dispatch: ReturnType<typeof createMcpDispatcher>;
  const readMediaFile = vi.fn();

  beforeEach(() => {
    vi.resetAllMocks();
    document.body.innerHTML = "";
    window.electron.readMediaFile = readMediaFile;
    state = new State();
    state.setEditorActive(true);
    state.setMaxTime(12000);
    dispatch = createMcpDispatcher(state);
  });

  function resource(type: "image" | "video" | "audio", index: number) {
    const el = document.createElement(type === "image" ? "img" : type);
    el.id = `${type}-${index}`;
    el.src = `data:${type}/${type === "image" ? "png" : "test"};base64,AA==`;
    document.body.append(el);
    return el;
  }

  it.each(["image", "video", "audio"] as const)("waits for %s panel mount and decoding before creating real elements", async (type) => {
    const url = `data:${type}/test;base64,AA==`;
    // Force index 1 to catch implementations that always use resource 0.
    if (type === "image") state.addImageResource("existing");
    else if (type === "video") state.addVideoResource("existing");
    else state.addAudioResource("existing");
    readMediaFile.mockResolvedValue({ success: true, type, dataUrl: url });
    let mount!: (el: HTMLElement) => void;
    let ready!: () => void;
    vi.mocked(waitForElementById).mockImplementation(() => new Promise(resolve => { mount = resolve; }));
    vi.mocked(waitForMediaReady).mockImplementation(() => new Promise(resolve => { ready = resolve; }));
    const pending = dispatch("add_media", { path: "/tmp/media.test" });
    await vi.waitFor(() => expect(waitForElementById).toHaveBeenCalledWith(`${type}-1`));
    expect(state.selectedMenuOption).toBe(type === "image" ? "Images" : type === "video" ? "Videos" : "Audios");
    expect(type === "image" ? state.images : type === "video" ? state.videos : state.audios).toEqual(["existing", url]);
    expect(state.editorElements).toHaveLength(0);
    const el = resource(type, 1);
    mount(el);
    await vi.waitFor(() => expect(waitForMediaReady).toHaveBeenCalledWith(el));
    expect(state.editorElements).toHaveLength(0);
    // Metadata becomes available only after mount and the readiness wait.
    if (type === "image") {
      Object.defineProperties(el, { naturalWidth: { value: 640 }, naturalHeight: { value: 320 } });
    } else {
      Object.defineProperty(el, "duration", { value: 3.25 });
      if (type === "video") Object.defineProperties(el, { videoWidth: { value: 640 }, videoHeight: { value: 320 } });
    }
    ready();
    const result = await pending;
    expect(readMediaFile).toHaveBeenCalledWith("/tmp/media.test");
    expect(result).toEqual({ type, ids: state.editorElements.map(e => e.id) });
    expect(state.editorElements.map(e => e.type)).toEqual(type === "video" ? ["video", "audio"] : [type]);
    expect(state.editorElements[0]).toMatchObject({
      timeFrame: { start: 0, end: type === "image" ? 12000 : 3250 },
      properties: { src: el.src },
    });
    if (type !== "audio") expect(state.editorElements[0].placement).toMatchObject({ width: 200, height: 100 });
  });

  it.each(["File does not exist", "Unsupported media type: .txt"])("surfaces IPC error: %s", async (error) => {
    readMediaFile.mockResolvedValue({ success: false, error });
    await expect(dispatch("add_media", { path: "/tmp/file" })).rejects.toThrow(error);
    expect(waitForElementById).not.toHaveBeenCalled();
    expect(state.images).toHaveLength(0);
    expect(state.editorElements).toHaveLength(0);
  });

  it("reports a mount timeout and releases the import lock", async () => {
    readMediaFile.mockResolvedValue({ success: true, type: "image", dataUrl: "data:image/png;base64,AA==" });
    vi.mocked(waitForElementById).mockResolvedValue(null);
    await expect(dispatch("add_media", { path: "/tmp/file.png" })).rejects.toThrow("never mounted");
    await expect(dispatch("add_media", { path: "/tmp/file.png" })).rejects.toThrow("never mounted");
    expect(state.editorElements).toHaveLength(0);
    expect(waitForMediaReady).not.toHaveBeenCalled();
  });

  it("rejects broken decoded media even when the readiness helper resolves on error", async () => {
    const el = resource("image", 0);
    readMediaFile.mockResolvedValue({ success: true, type: "image", dataUrl: el.src });
    vi.mocked(waitForElementById).mockResolvedValue(el);
    vi.mocked(waitForMediaReady).mockResolvedValue();
    await expect(dispatch("add_media", { path: "/tmp/broken.png" })).rejects.toThrow("Could not decode image");
    expect(state.editorElements).toHaveLength(0);
  });

  it("bounds decoding and rejects a concurrent panel-changing import", async () => {
    vi.useFakeTimers();
    try {
      const el = resource("audio", 0);
      readMediaFile.mockResolvedValue({ success: true, type: "audio", dataUrl: el.src });
      vi.mocked(waitForElementById).mockResolvedValue(el);
      vi.mocked(waitForMediaReady).mockImplementation(() => new Promise(() => {}));
      const pending = dispatch("add_media", { path: "/tmp/stalled.mp3" });
      const failure = expect(pending).rejects.toThrow("Timed out decoding");
      await expect(dispatch("add_media", { path: "/tmp/other.png" })).rejects.toThrow("Another media import");
      await vi.advanceTimersByTimeAsync(10000);
      await failure;
      expect(state.editorElements).toHaveLength(0);
    } finally { vi.useRealTimers(); }
  });

  it.each(["mp4", "webm"] as const)("infers %s format and awaits export completion", async (format) => {
    let finish!: () => void;
    const exportSpy = vi.spyOn(state.exportStore, "saveCanvasToVideoToPath")
      .mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    let settled = false;
    const pending = dispatch("export_video", { path: `/tmp/render.${format}` }).then(result => { settled = true; return result; });
    await Promise.resolve();
    expect(exportSpy).toHaveBeenCalledWith(`/tmp/render.${format}`, format);
    expect(settled).toBe(false);
    finish();
    await expect(pending).resolves.toEqual({ exported: true, path: `/tmp/render.${format}`, format });
  });

  it.each([
    { path: "relative.webm" }, { path: "/tmp/render.txt" },
    { path: "/tmp/render.mp4", format: "webm" }, { path: "/tmp/render.mp4", format: "gif" },
  ])("rejects invalid export destination or format %j before recording", async (args) => {
    const exportSpy = vi.spyOn(state.exportStore, "saveCanvasToVideoToPath");
    await expect(dispatch("export_video", args)).rejects.toThrow();
    expect(exportSpy).not.toHaveBeenCalled();
  });

  it("surfaces export recording/writing errors to the agent", async () => {
    vi.spyOn(state.exportStore, "saveCanvasToVideoToPath").mockRejectedValue(new Error("Permission denied"));
    await expect(dispatch("export_video", { path: "/tmp/render.mp4", format: "mp4" })).rejects.toThrow("Permission denied");
  });
});
