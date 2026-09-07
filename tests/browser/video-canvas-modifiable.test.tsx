import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render, waitFor, act } from "@testing-library/react";
import { StateContext } from "@/states";
import { State } from "@/states/state";
import type { VideoEditorElement, ImageEditorElement } from "@/types";

// Regression coverage for issue #11 ("the video might be not modifiable"):
// video elements used to be excluded from canvas drag/transform interactivity
// that image/text elements already had. These tests render the real Editor
// canvas (Konva Stage/Layer via react-konva) and assert video elements get
// the exact same draggable/transformable/clampable treatment as image
// elements, and that video properties (effects) are editable through the
// same panel used for image elements.

function makeVideoElement(id: string): VideoEditorElement {
  return {
    id,
    name: "Media(video) 1",
    type: "video",
    placement: { x: 10, y: 10, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
    timeFrame: { start: 0, end: 5000 },
    properties: {
      elementId: `video-${id}`,
      src: "test-video-src",
      effect: { type: "none" },
      muted: true,
    },
  };
}

function makeImageElement(id: string): ImageEditorElement {
  return {
    id,
    name: "Media(image) 1",
    type: "image",
    placement: { x: 10, y: 10, width: 100, height: 100, rotation: 0, scaleX: 1, scaleY: 1 },
    timeFrame: { start: 0, end: 5000 },
    properties: {
      elementId: `image-${id}`,
      src: "test-image-src",
      effect: { type: "none" },
    },
  };
}

describe("Video elements are draggable/transformable on the canvas, same as image elements", () => {
  it("registers a draggable, clamp-bound Konva node for a video element, matching image", async () => {
    const { default: Editor } = await import("../../renderer/pages/Editor");

    const state = new State();
    state.setEditorActive(true);
    state.setCanvasSize(800, 600);

    const video = makeVideoElement("vid-1");
    const image = makeImageElement("img-1");
    state.setEditorElements([video, image]);

    render(
      <StateContext.Provider value={state}>
        <Editor />
      </StateContext.Provider>
    );

    await waitFor(() => {
      expect(state.getKonvaNode("vid-1")).toBeDefined();
      expect(state.getKonvaNode("img-1")).toBeDefined();
    });

    const videoNode = state.getKonvaNode("vid-1")!;
    const imageNode = state.getKonvaNode("img-1")!;

    // Both must be draggable...
    expect(videoNode.draggable()).toBe(true);
    expect(imageNode.draggable()).toBe(true);

    // ...and both must have the same clamp-to-stage dragBoundFunc wired up
    // (not just "some" function - the video must not be left with the Konva
    // default identity dragBoundFunc while image gets the real clamp).
    expect(typeof videoNode.dragBoundFunc()).toBe("function");
    expect(typeof imageNode.dragBoundFunc()).toBe("function");

    const videoBound = videoNode.dragBoundFunc()!.call(videoNode, { x: -10000, y: -10000 });
    const imageBound = imageNode.dragBoundFunc()!.call(imageNode, { x: -10000, y: -10000 });
    // Clamped: dragging far off-stage must not leave the node at (-10000,-10000).
    expect(videoBound.x).toBeGreaterThan(-10000);
    expect(videoBound.y).toBeGreaterThan(-10000);
    expect(imageBound.x).toBeGreaterThan(-10000);
    expect(imageBound.y).toBeGreaterThan(-10000);
  });

  it("persists placement through the same onDragEnd/onTransformEnd path as image elements", async () => {
    const { default: Editor } = await import("../../renderer/pages/Editor");

    const state = new State();
    state.setEditorActive(true);
    state.setCanvasSize(800, 600);

    const video = makeVideoElement("vid-2");
    state.setEditorElements([video]);

    render(
      <StateContext.Provider value={state}>
        <Editor />
      </StateContext.Provider>
    );

    await waitFor(() => {
      expect(state.getKonvaNode("vid-2")).toBeDefined();
    });

    const node = state.getKonvaNode("vid-2")!;
    const updateSpy = vi.spyOn(state, "updateEditorElement");

    // Simulate a completed drag: Konva sets the node's position, then fires
    // "dragend" with e.target === node, exactly like a real user drag would.
    node.x(123);
    node.y(77);
    act(() => {
      node.fire("dragend", { target: node } as any, true);
    });

    await waitFor(() => {
      expect(updateSpy).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "vid-2",
          placement: expect.objectContaining({ x: 123, y: 77 }),
        })
      );
    });

    // Simulate a completed resize/rotate via the Transformer.
    node.scaleX(1.5);
    node.scaleY(2);
    node.rotation(30);
    act(() => {
      node.fire("transformend", { target: node } as any, true);
    });

    await waitFor(() => {
      expect(updateSpy).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "vid-2",
          placement: expect.objectContaining({ scaleX: 1.5, scaleY: 2, rotation: 30 }),
        })
      );
    });
  });

  it("attaches the Transformer (resize/rotate handles) to a selected video element, same as image", async () => {
    const { default: Editor } = await import("../../renderer/pages/Editor");

    const state = new State();
    state.setEditorActive(true);
    state.setCanvasSize(800, 600);

    const video = makeVideoElement("vid-3");
    state.setEditorElements([video]);

    render(
      <StateContext.Provider value={state}>
        <Editor />
      </StateContext.Provider>
    );

    await waitFor(() => {
      expect(state.getKonvaNode("vid-3")).toBeDefined();
    });

    act(() => {
      state.setSelectedElement(video);
    });

    await waitFor(() => {
      const node = state.getKonvaNode("vid-3")!;
      const transformer = node.getStage()?.findOne("Transformer") as any;
      expect(transformer).toBeTruthy();
      expect(transformer.nodes()).toContain(node);
    });
  });

  it("shows the Effects panel (property editing) for a selected video element, same as image", async () => {
    const { default: EffectsPanel } = await import(
      "../../renderer/pages/components/panels/EffectsPanel"
    );

    const state = new State();
    const video = makeVideoElement("vid-4");
    state.setSelectedElement(video);

    const { container } = render(
      <StateContext.Provider value={state}>
        <EffectsPanel />
      </StateContext.Provider>
    );

    await waitFor(() => {
      // EffectResource renders effect option buttons/labels for the selected element.
      expect(container.textContent).toMatch(/none|sepia|black|invert|saturate/i);
    });
  });
});
