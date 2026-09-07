import { makeAutoObservable } from "mobx";
import { FFmpeg } from "@ffmpeg/ffmpeg";
import { toBlobURL } from "@ffmpeg/util";
import fixWebmDuration from "webm-duration-fix";
import type { RootStore } from "../RootStore";
import { isEditorAudioElement } from "../state";

export class ExportStore {
  possibleVideoFormats: string[] = ["mp4", "webm"];
  selectedVideoFormat: "mp4" | "webm" = "mp4";

  constructor(private root: RootStore) {
    makeAutoObservable(this);
  }

  setVideoFormat(format: "mp4" | "webm") {
    this.selectedVideoFormat = format;
  }

  saveCanvasToVideoWithAudio() {
    this.saveCanvasToVideoWithAudioWebmMp4();
  }

  saveCanvasToVideoWithAudioWebmMp4() {
    const mp4 = this.selectedVideoFormat === "mp4";
    const layer = this.root.canvasStore.layer;

    if (!layer) {
      console.error("No Konva layer available for video export");
      return;
    }

    // Get the scene canvas from the Konva layer
    const htmlCanvas = layer.getCanvas()._canvas;
    const stream = htmlCanvas.captureStream(30) as MediaStream;

    const audioElements = this.root.elementStore.editorElements.filter(isEditorAudioElement);
    const audioStreams: MediaStream[] = [];
    // Track per-export connections so they can be torn down again once this
    // export finishes, instead of accumulating across repeated exports.
    const exportConnections: { sourceNode: MediaElementAudioSourceNode; dest: MediaStreamAudioDestinationNode }[] = [];

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

    let mixerContext: AudioContext | null = null;
    if (audioStreams.length > 0) {
      mixerContext = new AudioContext();
      const mixerDestination = mixerContext.createMediaStreamDestination();
      audioStreams.forEach((audioStream) => {
        const sourceNode = mixerContext!.createMediaStreamSource(audioStream);
        sourceNode.connect(mixerDestination);
      });
      stream.addTrack(mixerDestination.stream.getAudioTracks()[0]);
    }

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

    // Shared failure handler: video.play() rejecting and the MediaRecorder
    // erroring out are both paths that previously skipped teardown entirely,
    // leaking mixerContext/exportConnections and leaving playbackStore stuck
    // in the "playing" state with nothing shown to the user.
    const handleExportFailure = (error: unknown) => {
      console.error("Video export failed:", error);
      cleanupExportAudioGraph();
      this.root.playbackStore.setPlaying(false);
    };

    const video = document.createElement("video");
    video.srcObject = stream;
    video.height = 500;
    video.width = 800;

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

      mediaRecorder.onstop = async function () {
        cleanupExportAudioGraph();

        const blob = await fixWebmDuration(
          new Blob([...chunks], { type: "video/webm" })
        );

        if (mp4) {
          const data = new Uint8Array(await blob.arrayBuffer());
          const ffmpeg = new FFmpeg();
          const baseURL = "https://unpkg.com/@ffmpeg/core@0.12.2/dist/umd";
          await ffmpeg.load({
            coreURL: await toBlobURL(`${baseURL}/ffmpeg-core.js`, "text/javascript"),
            wasmURL: await toBlobURL(`${baseURL}/ffmpeg-core.wasm`, "application/wasm"),
          });
          await ffmpeg.writeFile("video.webm", data);
          await ffmpeg.exec([
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

          const output = await ffmpeg.readFile("video.mp4");
          const outputBlob = new Blob(
            [
              typeof output === "string"
                ? new TextEncoder().encode(output)
                : new Uint8Array(output),
            ],
            { type: "video/mp4" }
          );
          const outputUrl = URL.createObjectURL(outputBlob);
          const a = document.createElement("a");
          a.download = "video.mp4";
          a.href = outputUrl;
          a.click();
        } else {
          const url = URL.createObjectURL(blob);
          const a = document.createElement("a");
          a.download = "video.webm";
          a.href = url;
          a.click();
        }
      };

      mediaRecorder.start();
      setTimeout(() => {
        mediaRecorder.stop();
        this.root.playbackStore.setPlaying(false);
      }, this.root.playbackStore.maxTime);

      video.remove();
    }).catch(handleExportFailure);
  }
}
