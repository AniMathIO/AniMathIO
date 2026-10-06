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
