import { makeAutoObservable } from "mobx";
import type { RootStore } from "../RootStore";

export class AudioContextStore {
  audioContexts = new Map<
    string,
    { context: AudioContext; sourceNode: MediaElementAudioSourceNode }
  >();

  constructor(private root: RootStore) {
    makeAutoObservable(this, {
      audioContexts: false,
    });
  }

  getAudioContext(audioElement: HTMLAudioElement) {
    let entry = this.audioContexts.get(audioElement.id);
    if (!entry) {
      const ctx = new AudioContext();
      const sourceNode = ctx.createMediaElementSource(audioElement);
      // Route the element's audio to the normal speakers as well, so that
      // rerouting the output into the Web Audio graph (for export) doesn't
      // permanently silence preview playback of this element.
      sourceNode.connect(ctx.destination);
      entry = { context: ctx, sourceNode };
      this.audioContexts.set(audioElement.id, entry);
    }
    // A context created outside a user gesture (or resurrected after being
    // suspended by the browser) starts/returns in "suspended" state, which
    // silently produces no audio output. Resume it defensively every time
    // this entry is handed out.
    if (entry.context.state === "suspended") {
      entry.context.resume().catch(() => {
        // Resume can reject if the context was closed concurrently; ignore.
      });
    }
    return entry;
  }

  /**
   * Remove and close the cached AudioContext (if any) bound to the given
   * element id. Called when an editor element is deleted so a later element
   * that reuses the same id doesn't inherit a source node bound to a
   * detached DOM node, and so we don't leak AudioContexts over time.
   */
  releaseAudioContext(elementId: string) {
    const entry = this.audioContexts.get(elementId);
    if (entry) {
      this.audioContexts.delete(elementId);
      try {
        entry.context.close();
      } catch {
        // AudioContext may already be closed; ignore.
      }
    }
  }

  /**
   * Release cached AudioContexts whose <audio> element is no longer in the
   * document. Called on project switch (resetForNewProject / deserialize).
   *
   * Deliberately does NOT close contexts for elements that are still mounted.
   * A MediaElementAudioSourceNode binding is permanent for the lifetime of its
   * element: once bound, that element can never be attached to another context,
   * even after the original is closed. React reconciles the <audio> nodes by
   * element id, so re-opening a project reuses the very same DOM nodes - closing
   * their contexts here would make the next export throw InvalidStateError out
   * of createMediaElementSource, killing the export before it starts.
   *
   * Contexts for elements that survive a switch are therefore kept (they are
   * still the correct context for that element). Ones whose element has since
   * unmounted are closed, so successive switches converge instead of
   * accumulating against Chromium's per-document AudioContext cap.
   */
  releaseDetached() {
    this.audioContexts.forEach((entry, elementId) => {
      if (typeof document !== "undefined" && document.getElementById(elementId)) {
        return;
      }
      this.audioContexts.delete(elementId);
      try {
        entry.context.close();
      } catch {
        // AudioContext may already be closed; ignore.
      }
    });
  }
}
