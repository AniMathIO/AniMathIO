import type { RootStore } from "../RootStore";

/**
 * Handles global keyboard shortcuts for the editor (delete, arrow-key nudge/seek,
 * copy/paste, play/pause). Not an observable store - just encapsulates the
 * window keydown listener and dispatch logic that used to live on RootStore.
 */
export class KeyboardShortcutService {
  constructor(private root: RootStore) {
    this.handleKeyboardShortcut = this.handleKeyboardShortcut.bind(this);
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
