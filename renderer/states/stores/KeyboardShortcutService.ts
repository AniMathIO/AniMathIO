import type { RootStore } from "../RootStore";

/**
 * Handles global keyboard shortcuts for the editor (delete, arrow-key nudge/seek,
 * copy/paste, play/pause). Not an observable store - just encapsulates the
 * window keydown listener and dispatch logic that used to live on RootStore.
 */
export class KeyboardShortcutService {
  private attached = false;

  constructor(private root: RootStore) {
    this.handleKeyboardShortcut = this.handleKeyboardShortcut.bind(this);
  }

  /**
   * Registers the global keydown listener. Scoped to be called by whichever
   * component owns the "editor is active" lifecycle (Editor.tsx), not
   * RootStore's constructor - the store is shared by the Dashboard, which has
   * no canvas and shouldn't respond to these shortcuts. Idempotent: calling
   * this while already attached is a no-op, so remounts/StrictMode don't
   * register duplicate listeners.
   */
  attach() {
    if (this.attached || typeof window === "undefined") return;
    window.addEventListener("keydown", this.handleKeyboardShortcut);
    this.attached = true;
  }

  /** Removes the global keydown listener. Safe to call when not attached. */
  detach() {
    if (!this.attached || typeof window === "undefined") return;
    window.removeEventListener("keydown", this.handleKeyboardShortcut);
    this.attached = false;
  }

  handleKeyboardShortcut(event: KeyboardEvent) {
    const target = event.target as HTMLElement;
    const isInputElement =
      target.tagName === "INPUT" ||
      target.tagName === "TEXTAREA" ||
      target.isContentEditable ||
      target.tagName === "SELECT";

    if (isInputElement) return;

    switch (event.key) {
      case "Delete":
        if (event.ctrlKey || event.metaKey) {
          if (this.root.selectedElement) {
            this.root.deleteSelectedObjects([this.root.selectedElement]);
          }
          event.preventDefault();
        }
        break;
      case "ArrowUp":
      case "ArrowDown":
      case "ArrowLeft":
      case "ArrowRight":
        if (event.ctrlKey || event.metaKey) {
          this.root.moveSelectedObject(event.key);
          event.preventDefault();
        } else {
          this.root.skipInTime(event.key);
          event.preventDefault();
        }
        break;
      case "c":
        if ((event.ctrlKey || event.metaKey) && event.altKey) {
          this.root.copyObject();
          event.preventDefault();
        }
        break;
      case "v":
        if ((event.ctrlKey || event.metaKey) && event.altKey) {
          this.root.pasteObject();
          event.preventDefault();
        }
        break;
      case " ":
        this.root.setPlaying(!this.root.playing);
        break;
      default:
        break;
    }
  }
}
