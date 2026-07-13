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
    this.root.elementStore.editorElements
      .filter((element): element is VideoEditorElement => element.type === "video")
      .forEach((element) => {
        const video = document.getElementById(element.properties.elementId);
        if (isHtmlVideoElement(video)) {
          const videoTime = (this.currentTimeInMs - element.timeFrame.start) / 1000;
          video.currentTime = videoTime;
          if (this.playing) {
            video.play();
          } else {
            video.pause();
          }
        }
      });
  }

  updateAudioElements() {
    this.root.elementStore.editorElements
      .filter((element): element is AudioEditorElement => element.type === "audio")
      .forEach((element) => {
        const audio = document.getElementById(element.properties.elementId);
        if (isHtmlAudioElement(audio)) {
          const audioTime = (this.currentTimeInMs - element.timeFrame.start) / 1000;
          audio.currentTime = audioTime;
          if (this.playing) {
            audio.play();
          } else {
            audio.pause();
          }
        }
      });
  }
}
