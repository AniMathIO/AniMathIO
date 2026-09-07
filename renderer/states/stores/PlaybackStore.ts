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
  /** Playhead position at which media elements were last synced (drift correction). */
  private lastMediaSyncTimeMs: number = 0;
  /** Playhead position at the previous playback tick, used for boundary detection. */
  private lastTickTimeMs: number = 0;

  /**
   * How often media elements are re-synced while playing when no clip boundary
   * is crossed. Reassigning `currentTime` every frame causes audible stutter,
   * so plain drift correction is deliberately coarse.
   */
  private static readonly DRIFT_SYNC_INTERVAL_MS = 250;

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
    this.lastTickTimeMs = this.currentTimeInMs;
    this.lastMediaSyncTimeMs = this.currentTimeInMs;
    this.syncMediaElements();
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
      // The tick above ran past maxTime, which hid every element. Stop first,
      // then rewind *through* updateTimeTo so node visibility is recomputed and
      // redrawn for t=0 - otherwise the playhead returns to the start but the
      // canvas stays blank until the user scrubs or plays again.
      this.setPlaying(false);
      this.setCurrentKeyFrame(0);
      this.updateTimeTo(0);
      this.syncMediaElements();
    } else {
      requestAnimationFrame(() => this.playFrames());
    }
  }

  updateTimeTo(newTime: number) {
    const previousTime = this.lastTickTimeMs;
    this.setCurrentTimeInMs(newTime);
    // The playhead is quantised to whole frames, and that quantised value is
    // what the media sync methods read — so boundary detection must use it too.
    const tickTime = this.currentTimeInMs;
    this.root.animationStore.animationTimeLine?.seek(newTime);

    // Update Konva node visibility based on timeframe
    const { editorElements, konvaNodes } = this.root.elementStore;
    editorElements.forEach((e) => {
      const node = konvaNodes.get(e.id);
      if (!node) return;
      const isInside = e.timeFrame.start <= tickTime && tickTime <= e.timeFrame.end;
      node.visible(isInside);
    });

    this.root.canvasStore.layer?.batchDraw();

    // Keep audio *and* video elements in sync with the frame clock during
    // ongoing playback. Two different cadences:
    //   - a clip boundary crossed on this tick must be honoured immediately,
    //     otherwise clips start/stop up to a throttle window late;
    //   - otherwise sync only every DRIFT_SYNC_INTERVAL_MS, because
    //     reassigning currentTime every frame causes playback stutter.
    if (this.playing) {
      const crossedBoundary = this.crossesMediaBoundary(previousTime, tickTime);
      const driftDue =
        Math.abs(tickTime - this.lastMediaSyncTimeMs) >= PlaybackStore.DRIFT_SYNC_INTERVAL_MS;
      if (crossedBoundary || driftDue) {
        this.lastMediaSyncTimeMs = tickTime;
        this.syncMediaElements();
      }
    } else {
      this.lastMediaSyncTimeMs = tickTime;
    }

    this.lastTickTimeMs = tickTime;
  }

  /** Re-evaluate every media element against the current playhead position. */
  private syncMediaElements() {
    this.updateVideoElements();
    this.updateAudioElements();
  }

  /**
   * Whether any audio/video clip's active state differs between the two ends of
   * the interval the playhead just traversed - i.e. whether this tick needs to
   * start or stop something.
   *
   * This asks the same question `updateVideoElements`/`updateAudioElements` ask
   * (`start <= t && t <= end`) rather than testing whether a boundary *value*
   * falls in the interval. Those two are not equivalent: because the playhead is
   * quantised to whole frames, a clip end that is a multiple of 50ms lands
   * exactly on a frame, and at that frame the clip is still active (`t === end`
   * is inside). The stop is only due on the *next* frame - which a boundary-value
   * test misses, leaving the clip running until the next drift tick.
   *
   * Direction-agnostic, so seeking backwards works too.
   */
  private crossesMediaBoundary(fromTime: number, toTime: number): boolean {
    const lo = Math.min(fromTime, toTime);
    const hi = Math.max(fromTime, toTime);
    if (lo === hi) return false;

    return this.root.elementStore.editorElements.some((element) => {
      if (element.type !== "video" && element.type !== "audio") return false;
      const { start, end } = element.timeFrame;

      const activeAtLo = start <= lo && lo <= end;
      const activeAtHi = start <= hi && hi <= end;
      if (activeAtLo !== activeAtHi) return true;

      // A clip shorter than the gap between samples is inactive at both ends
      // yet still needs handling; only reachable for degenerate clips or an
      // unusually long tick.
      return start > lo && end <= hi;
    });
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
