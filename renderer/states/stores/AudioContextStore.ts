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
      entry = { context: ctx, sourceNode };
      this.audioContexts.set(audioElement.id, entry);
    }
    return entry;
  }
}
