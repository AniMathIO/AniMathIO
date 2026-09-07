import React from "react";
import { describe, it, expect } from "vitest";
import { render, act, waitFor } from "@testing-library/react";
import { StateContext } from "@/states";
import { State } from "@/states/state";

// Home renders <Dashboard /> and <Editor /> as siblings, so EditorInner stays
// mounted (rendering null) the entire time the user is on the Dashboard.
// Attaching the global keydown listener on mount alone therefore isn't enough:
// shortcuts would be live on the Dashboard, where arrow keys would hijack list
// navigation and Space would start a phantom playback loop. These tests drive
// the real Editor component and assert the listener follows isEditorActive.

function dispatchKey(key: string) {
  return act(() => {
    window.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  });
}

describe("global keyboard shortcuts are scoped to an open editor", () => {
  it("ignores shortcuts while the editor is inactive (user is on the Dashboard)", async () => {
    const { default: Editor } = await import("../../renderer/pages/Editor");

    const state = new State();
    // isEditorActive is false by default - this is the Dashboard case.
    render(
      <StateContext.Provider value={state}>
        <Editor />
      </StateContext.Provider>
    );

    await dispatchKey("ArrowRight");
    await dispatchKey(" ");

    expect(state.currentTimeInMs).toBe(0);
    expect(state.playing).toBe(false);
  });

  it("acts on shortcuts once the editor is active", async () => {
    const { default: Editor } = await import("../../renderer/pages/Editor");

    const state = new State();
    state.setEditorActive(true);
    state.setCanvasSize(800, 600);

    render(
      <StateContext.Provider value={state}>
        <Editor />
      </StateContext.Provider>
    );

    await waitFor(() => expect(state.isEditorActive).toBe(true));

    await dispatchKey("ArrowRight");

    // skipForward() advances by 10s, clamped to maxTime.
    expect(state.currentTimeInMs).toBe(10000);
  });

  it("stops acting on shortcuts when the editor closes while still mounted", async () => {
    const { default: Editor } = await import("../../renderer/pages/Editor");

    const state = new State();
    state.setEditorActive(true);
    state.setCanvasSize(800, 600);

    render(
      <StateContext.Provider value={state}>
        <Editor />
      </StateContext.Provider>
    );

    await waitFor(() => expect(state.isEditorActive).toBe(true));
    await dispatchKey("ArrowRight");
    expect(state.currentTimeInMs).toBe(10000);

    // Returning to the Dashboard does not unmount EditorInner - it only flips
    // this flag and renders null - so this is the transition the effect's
    // dependency array exists for, distinct from the unmount case below.
    await act(async () => {
      state.setEditorActive(false);
    });

    const timeBefore = state.currentTimeInMs;
    await dispatchKey("ArrowRight");
    expect(state.currentTimeInMs).toBe(timeBefore);
  });

  it("stops acting on shortcuts after the editor unmounts", async () => {
    const { default: Editor } = await import("../../renderer/pages/Editor");

    const state = new State();
    state.setEditorActive(true);
    state.setCanvasSize(800, 600);

    const { unmount } = render(
      <StateContext.Provider value={state}>
        <Editor />
      </StateContext.Provider>
    );

    await waitFor(() => expect(state.isEditorActive).toBe(true));
    unmount();

    const timeBefore = state.currentTimeInMs;
    await dispatchKey("ArrowRight");

    expect(state.currentTimeInMs).toBe(timeBefore);
  });
});
