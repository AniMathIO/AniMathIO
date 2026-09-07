import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render, waitFor, fireEvent } from "@testing-library/react";
import { StateContext } from "@/states";
import { State } from "@/states/state";
import type { AudioEditorElement } from "@/types";

// Isolate the timeline drag mechanics from real waveform rendering/decoding.
vi.mock("@wavesurfer/react", () => ({
  default: () => null,
}));

function makeAudioElement(): AudioEditorElement {
  return {
    id: "audio-el-1",
    name: "Media(audio) 1",
    type: "audio",
    placement: {
      x: 0,
      y: 0,
      width: 100,
      height: 100,
      rotation: 0,
      scaleX: 1,
      scaleY: 1,
    },
    timeFrame: { start: 1000, end: 5000 },
    properties: {
      elementId: "audio-audio-el-1",
      src: "test-audio-src",
      volume: 1,
      muted: false,
    },
  };
}

describe("Timeline dragging for audio elements", () => {
  it("does not mark the audio timeline item as non-draggable (regression: audio used to be excluded)", async () => {
    const { default: TimeFrame } = await import(
      "../../renderer/pages/components/partials/timeline/Timeframe"
    );

    const state = new State();
    state.setMaxTime(10000);
    const element = makeAudioElement();

    const { container } = render(
      <StateContext.Provider value={state}>
        <TimeFrame element={element} />
      </StateContext.Provider>
    );

    await waitFor(() => {
      expect(container.querySelector(".cursor-no-drop")).toBeNull();
      expect(container.querySelector(".cursor-ew-resize")).not.toBeNull();
      expect(container.querySelector(".cursor-col-resize")).not.toBeNull();
    });
  });

  it("updates an audio element's time frame when its timeline body is dragged", async () => {
    const { default: TimeFrame } = await import(
      "../../renderer/pages/components/partials/timeline/Timeframe"
    );

    const state = new State();
    state.setMaxTime(10000);
    const element = makeAudioElement();
    const updateSpy = vi
      .spyOn(state, "updateEditorElementTimeFrame")
      .mockResolvedValue();

    const { container } = render(
      <StateContext.Provider value={state}>
        <TimeFrame element={element} />
      </StateContext.Provider>
    );

    let dragHandle: Element | null = null;
    await waitFor(() => {
      dragHandle = container.querySelector(".cursor-col-resize");
      expect(dragHandle).not.toBeNull();
    });

    fireEvent.mouseDown(dragHandle!, { clientX: 0 });
    fireEvent.mouseMove(window, { clientX: 200 });
    fireEvent.mouseUp(window, { clientX: 200 });

    await waitFor(() => {
      expect(updateSpy).toHaveBeenCalled();
    });
  });

  it("updates an audio element's start handle when dragged", async () => {
    const { default: TimeFrame } = await import(
      "../../renderer/pages/components/partials/timeline/Timeframe"
    );

    const state = new State();
    state.setMaxTime(10000);
    const element = makeAudioElement();
    const updateSpy = vi
      .spyOn(state, "updateEditorElementTimeFrame")
      .mockResolvedValue();

    const { container } = render(
      <StateContext.Provider value={state}>
        <TimeFrame element={element} />
      </StateContext.Provider>
    );

    let handles: NodeListOf<Element> | null = null;
    await waitFor(() => {
      handles = container.querySelectorAll(".cursor-ew-resize");
      expect(handles!.length).toBeGreaterThanOrEqual(2);
    });

    const startHandle = handles![0];
    fireEvent.mouseDown(startHandle, { clientX: 0 });
    fireEvent.mouseMove(window, { clientX: 100 });
    fireEvent.mouseUp(window, { clientX: 100 });

    await waitFor(() => {
      expect(updateSpy).toHaveBeenCalledWith(
        expect.objectContaining({ id: element.id }),
        expect.objectContaining({ start: expect.any(Number) })
      );
    });
  });
});
