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
   * Close and clear every cached AudioContext. Called on project switch
   * (resetForNewProject / deserialize) so contexts bound to the previous
   * project's (now-detached) audio elements don't linger for the rest of
   * the session and eat into Chromium's hard per-document AudioContext cap.
   */
  releaseAll() {
    this.audioContexts.forEach((entry) => {
      try {
        entry.context.close();
      } catch {
        // AudioContext may already be closed; ignore.
      }
    });
    this.audioContexts.clear();
  }
}
