import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { KeyboardShortcutService } from "../../../../renderer/states/stores/KeyboardShortcutService";
import type { RootStore } from "../../../../renderer/states/RootStore";

// KeyboardShortcutService.handleKeyboardShortcut only touches a handful of
// RootStore members, so a minimal fake is enough to drive it directly
// without pulling in the whole store graph.
function makeFakeRoot() {
  return {
    selectedElement: null,
    deleteSelectedObjects: vi.fn(),
    moveSelectedObject: vi.fn(),
    skipInTime: vi.fn(),
    copyObject: vi.fn(),
    pasteObject: vi.fn(),
    playing: false,
    setPlaying: vi.fn(),
  } as unknown as RootStore;
}

function pressSpace() {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: " " }));
}

describe("KeyboardShortcutService", () => {
  let root: ReturnType<typeof makeFakeRoot>;
  let service: KeyboardShortcutService;

  beforeEach(() => {
    root = makeFakeRoot();
    service = new KeyboardShortcutService(root);
  });

  afterEach(() => {
    // Belt-and-suspenders: make sure no test leaks a listener into the next one.
    service.detach();
  });

  it("is inert before attach() - no listener registered yet", () => {
    pressSpace();
    expect(root.setPlaying).not.toHaveBeenCalled();
  });

  it("dispatches shortcuts once attach() has been called", () => {
    service.attach();
    pressSpace();
    expect(root.setPlaying).toHaveBeenCalledTimes(1);
    expect(root.setPlaying).toHaveBeenCalledWith(true);
  });

  it("is inert again after detach()", () => {
    service.attach();
    service.detach();
    pressSpace();
    expect(root.setPlaying).not.toHaveBeenCalled();
  });

  it("attach() is idempotent - calling it twice registers only one listener", () => {
    service.attach();
    service.attach();
    pressSpace();
    expect(root.setPlaying).toHaveBeenCalledTimes(1);
  });

  it("detach() without a prior attach() is a no-op", () => {
    expect(() => service.detach()).not.toThrow();
    pressSpace();
    expect(root.setPlaying).not.toHaveBeenCalled();
  });
});
