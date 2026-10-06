import { makeAutoObservable } from "mobx";
import { FFmpeg } from "@ffmpeg/ffmpeg";
import { toBlobURL } from "@ffmpeg/util";
import fixWebmDuration from "webm-duration-fix";
import type { RootStore } from "../RootStore";
import { isEditorAudioElement } from "../state";

export class ExportStore {
  possibleVideoFormats: string[] = ["mp4", "webm"];
  selectedVideoFormat: "mp4" | "webm" = "mp4";
  private exporting = false;

  constructor(private root: RootStore) {
    makeAutoObservable(this);
  }

  setVideoFormat(format: "mp4" | "webm") {
    this.selectedVideoFormat = format;
  }

  saveCanvasToVideoWithAudio() {
    this.saveCanvasToVideoWithAudioWebmMp4();
  }

  /** Resolves after recording, conversion, and writing; never downloads a file. */
  saveCanvasToVideoToPath(path: string, format: "mp4" | "webm"): Promise<void> {
    return new Promise((resolve, reject) => {
      this.saveCanvasToVideoWithAudioWebmMp4(async (blob) => {
        const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
        const result = await window.electron.writeVideoFile(path, bytes);
        if (!result?.success) throw new Error(result?.error ?? `Could not write video to "${path}".`);
      }, resolve, reject, format);
    });
  }

  saveCanvasToVideoWithAudioWebmMp4(
    deliver?: (blob: Blob) => Promise<void>,
    onSuccess?: () => void,
    onFailure?: (error: unknown) => void,
    format = this.selectedVideoFormat
  ) {
    if (this.exporting) {
      onFailure?.(new Error("Another video export is in progress. Wait for it to finish."));
      return;
    }
    const mp4 = format === "mp4";
    const layer = this.root.canvasStore.layer;

    if (!layer) {
      console.error("No Konva layer available for video export");
      onFailure?.(new Error("No Konva layer available for video export."));
      return;
    }
    this.exporting = true;

    // Include synchronous capture/audio setup errors in the same completion
    // contract as asynchronous recording and conversion failures.
    let failExport: ((error: unknown) => void) | undefined;
    try {
      // Get the scene canvas from the Konva layer
      const htmlCanvas = layer.getCanvas()._canvas;
      const stream = htmlCanvas.captureStream(30) as MediaStream;

      const audioElements = this.root.elementStore.editorElements.filter(isEditorAudioElement);
      const audioStreams: MediaStream[] = [];
      // Track per-export connections so they can be torn down again once this
      // export finishes, instead of accumulating across repeated exports.
      const exportConnections: { sourceNode: MediaElementAudioSourceNode; dest: MediaStreamAudioDestinationNode }[] = [];

      let mixerContext: AudioContext | null = null;

      // Guard against running the teardown twice (e.g. once from a failure
      // path and once from mediaRecorder.onstop, or from two failure paths
      // racing each other) — every step inside is already individually
      // try/catch-guarded, but this avoids doing the work (and logging) twice.
      let cleanedUp = false;
      const cleanupExportAudioGraph = () => {
        if (cleanedUp) return;
        cleanedUp = true;
        exportConnections.forEach(({ sourceNode, dest }) => {
          try {
            sourceNode.disconnect(dest);
          } catch {
            // Already disconnected; ignore.
          }
        });
        if (mixerContext) {
          try {
            mixerContext.close();
          } catch {
            // Already closed; ignore.
          }
        }
      };

      // A MediaRecorder error fires `stop` after `error`, so onstop must know the
      // export already failed - otherwise it produces a file out of whatever
      // partial chunks exist and downloads it as if the export had succeeded.
      let exportFailed = false;
      let stopTimer: ReturnType<typeof setTimeout> | null = null;

      const video = document.createElement("video");
      video.srcObject = stream;
      video.height = 500;
      video.width = 800;

      // Shared failure handler: video.play() rejecting and the MediaRecorder
      // erroring out are both paths that previously skipped teardown entirely,
      // leaking mixerContext/exportConnections and leaving playbackStore stuck
      // in the "playing" state with nothing shown to the user.
      const handleExportFailure = (error: unknown) => {
        if (exportFailed) return;
        exportFailed = true;
        this.exporting = false;
        console.error("Video export failed:", error);
        if (stopTimer !== null) {
          clearTimeout(stopTimer);
          stopTimer = null;
        }
        cleanupExportAudioGraph();
        // Only the success path reaches video.remove(); without this the element
        // keeps srcObject pointing at the captured canvas/mixer stream.
        video.srcObject = null;
        video.remove();
        this.root.playbackStore.setPlaying(false);
        onFailure?.(error);
      };

      failExport = handleExportFailure;

      stream.getAudioTracks().forEach((track) => stream.removeTrack(track));

      audioElements.forEach((audio) => {
        const audioElement = document.getElementById(audio.properties.elementId) as HTMLAudioElement;
        if (audioElement) {
          const { context, sourceNode } = this.root.getAudioContext(audioElement);
          const dest = context.createMediaStreamDestination();
          sourceNode.connect(dest);
          exportConnections.push({ sourceNode, dest });
          audioStreams.push(dest.stream);
        }
      });

      if (audioStreams.length > 0) {
        mixerContext = new AudioContext();
        const mixerDestination = mixerContext.createMediaStreamDestination();
        audioStreams.forEach((audioStream) => {
          const sourceNode = mixerContext!.createMediaStreamSource(audioStream);
          sourceNode.connect(mixerDestination);
        });
        stream.addTrack(mixerDestination.stream.getAudioTracks()[0]);
      }

      // Seek to start and start playing for export
      this.root.playbackStore.handleSeek(0);
      this.root.playbackStore.setPlaying(true);

      video.play().then(() => {
        const mediaRecorder = new MediaRecorder(stream);
        const chunks: Blob[] = [];

        mediaRecorder.ondataavailable = function (e) {
          chunks.push(e.data);
        };

        mediaRecorder.onerror = (event: Event) => {
          handleExportFailure((event as any)?.error ?? event);
        };

        // Wrapped rather than assigned as an `async function` directly: nothing
        // awaits an event handler, so every await below (notably ffmpeg.load(),
        // which fetches its core from unpkg at runtime on the default mp4 path)
        // would otherwise reject into the void, leaving the user with no file and
        // no error while the UI looks like the export succeeded.
        mediaRecorder.onstop = () => {
          void (async () => {
            cleanupExportAudioGraph();

            // An errored recorder still fires `stop`; emitting a file from the
            // partial chunks would hand the user a corrupt download and no
            // indication anything went wrong.
            if (exportFailed) return;

            const blob = await fixWebmDuration(
              new Blob([...chunks], { type: "video/webm" })
            );

            if (exportFailed) return;

            if (mp4) {
              const data = new Uint8Array(await blob.arrayBuffer());
              const ffmpeg = new FFmpeg();
              const baseURL = "https://unpkg.com/@ffmpeg/core@0.12.2/dist/umd";
              await ffmpeg.load({
                coreURL: await toBlobURL(`${baseURL}/ffmpeg-core.js`, "text/javascript"),
                wasmURL: await toBlobURL(`${baseURL}/ffmpeg-core.wasm`, "application/wasm"),
              });
              await ffmpeg.writeFile("video.webm", data);
              const exitCode = await ffmpeg.exec([
                "-y",
                "-i",
                "video.webm",
                "-c:v",
                "libx264",
                "-preset",
                "superfast",
                "-crf",
                "24",
                "-c:a",
                "aac",
                "-b:a",
                "64k",
                "-movflags",
                "+faststart",
                "video.mp4",
              ]);
              if (exitCode !== 0) throw new Error(`MP4 conversion failed (FFmpeg exit code ${exitCode}).`);

              const output = await ffmpeg.readFile("video.mp4");
              const outputBlob = new Blob(
                [
                  typeof output === "string"
                    ? new TextEncoder().encode(output)
                    : new Uint8Array(output),
                ],
                { type: "video/mp4" }
              );
              if (exportFailed) return;
              if (deliver) {
                await deliver(outputBlob);
              } else {
                const outputUrl = URL.createObjectURL(outputBlob);
                const a = document.createElement("a");
                a.download = "video.mp4";
                a.href = outputUrl;
                a.click();
              }
            } else {
              if (deliver) {
                await deliver(blob);
              } else {
                const url = URL.createObjectURL(blob);
                const a = document.createElement("a");
                a.download = "video.webm";
                a.href = url;
                a.click();
              }
            }
            this.exporting = false;
            onSuccess?.();
          })().catch(handleExportFailure);
        };

        mediaRecorder.start();
        stopTimer = setTimeout(() => {
          stopTimer = null;
          // An errored recorder is already "inactive"; stopping it again throws
          // InvalidStateError out of a timer callback, where nothing catches it.
          if (mediaRecorder.state !== "inactive") mediaRecorder.stop();
          this.root.playbackStore.setPlaying(false);
        }, this.root.playbackStore.maxTime);

        video.remove();
      }).catch(handleExportFailure);
    } catch (error) {
      if (failExport) {
        failExport(error);
      } else {
        this.exporting = false;
        console.error("Video export failed:", error);
        onFailure?.(error);
      }
    }
  }
}
