import { makeAutoObservable } from "mobx";
import type { RootStore } from "../RootStore";
import { VideoEditorElement, AudioEditorElement } from "@/types";
import { isHtmlVideoElement, isHtmlAudioElement } from "@/utils";

export class PlaybackStore {
  playing: boolean = false;
  currentKeyFrame: number = 0;
  fps: number = 60;
  maxTime: number = 30 * 1000;
  startedTime: number = 0;
  startedTimePlay: number = 0;
  private lastAudioSyncTimeMs: number = 0;

  constructor(private root: RootStore) {
    makeAutoObservable(this);
  }

  get currentTimeInMs() {
    return (this.currentKeyFrame * 1000) / this.fps;
  }

  setCurrentTimeInMs(time: number) {
    this.currentKeyFrame = Math.floor((time / 1000) * this.fps);
  }

  setCurrentKeyFrame(frame: number) {
    this.currentKeyFrame = frame;
  }

  setMaxTime(maxTime: number) {
    this.maxTime = maxTime;
  }

  setPlaying(playing: boolean) {
    this.playing = playing;
    this.updateVideoElements();
    this.updateAudioElements();
    if (playing) {
      this.startedTime = Date.now();
      this.startedTimePlay = this.currentTimeInMs;
      requestAnimationFrame(() => this.playFrames());
    }
  }

  playFrames() {
    if (!this.playing) return;
    const elapsedTime = Date.now() - this.startedTime;
    const newTime = this.startedTimePlay + elapsedTime;
    this.updateTimeTo(newTime);
    if (newTime > this.maxTime) {
      this.currentKeyFrame = 0;
      this.setPlaying(false);
    } else {
      requestAnimationFrame(() => this.playFrames());
    }
  }

  updateTimeTo(newTime: number) {
    this.setCurrentTimeInMs(newTime);
    this.root.animationStore.animationTimeLine?.seek(newTime);

    // Update Konva node visibility based on timeframe
    const { editorElements, konvaNodes } = this.root.elementStore;
    editorElements.forEach((e) => {
      const node = konvaNodes.get(e.id);
      if (!node) return;
      const isInside = e.timeFrame.start <= newTime && newTime <= e.timeFrame.end;
      node.visible(isInside);
    });

    this.root.canvasStore.layer?.batchDraw();

    // Keep audio elements in sync with the frame clock during ongoing
    // playback. Throttled (rather than on every frame) to avoid stutter
    // from constantly reassigning audio.currentTime.
    if (this.playing) {
      if (Math.abs(newTime - this.lastAudioSyncTimeMs) >= 250) {
        this.lastAudioSyncTimeMs = newTime;
        this.updateAudioElements();
      }
    } else {
      this.lastAudioSyncTimeMs = newTime;
    }
  }

  handleSeek(seek: number) {
    if (this.playing) this.setPlaying(false);
    this.updateTimeTo(seek);
    this.updateVideoElements();
    this.updateAudioElements();
  }

  skipForward() {
    const nextTime = this.currentTimeInMs + 10000;
    this.handleSeek(Math.min(nextTime, this.maxTime));
  }

  skipBackward() {
    const nextTime = this.currentTimeInMs - 10000;
    this.handleSeek(Math.max(nextTime, 0));
  }

  skipToStart() {
    this.handleSeek(0);
  }

  skipToEnd() {
    this.handleSeek(this.maxTime);
  }

  skipInTime(key: string) {
    if (key === "ArrowLeft") this.skipBackward();
    if (key === "ArrowRight") this.skipForward();
  }

  updateVideoElements() {
    const t = this.currentTimeInMs;
    this.root.elementStore.editorElements
      .filter((element): element is VideoEditorElement => element.type === "video")
      .forEach((element) => {
        const video = document.getElementById(element.properties.elementId);
        if (!isHtmlVideoElement(video)) return;

        const { start, end } = element.timeFrame;
        const isInside = start <= t && t <= end;
        if (!isInside) {
          video.pause();
          video.currentTime = 0;
          return;
        }

        const videoTime = (t - start) / 1000;
        if (Math.abs(video.currentTime - videoTime) > 0.15) {
          video.currentTime = videoTime;
        }
        if (this.playing) {
          // Treat an unknown `paused` state (e.g. in tests) as "not playing"
          // so we don't skip play(); avoid re-invoking play() on an element
          // that's already actively playing.
          if (video.paused !== false) video.play();
        } else {
          video.pause();
        }
      });
  }

  updateAudioElements() {
    const t = this.currentTimeInMs;
    this.root.elementStore.editorElements
      .filter((element): element is AudioEditorElement => element.type === "audio")
      .forEach((element) => {
        const audio = document.getElementById(element.properties.elementId);
        if (!isHtmlAudioElement(audio)) return;

        const { start, end } = element.timeFrame;
        const isInside = start <= t && t <= end;
        if (!isInside) {
          audio.pause();
          audio.currentTime = 0;
          return;
        }

        const audioTime = (t - start) / 1000;
        if (Math.abs(audio.currentTime - audioTime) > 0.15) {
          audio.currentTime = audioTime;
        }
        if (this.playing) {
          if (audio.paused !== false) audio.play();
        } else {
          audio.pause();
        }
      });
  }
}
