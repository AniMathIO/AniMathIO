import { makeAutoObservable } from "mobx";
import { createTimeline } from "animejs";
import * as pako from "pako";
import type { RootStore } from "../RootStore";
import { EditorElement, AudioEditorElement } from "@/types";

export class ProjectStore {
  currentProjectFilePath: string | null = null;
  currentProjectFileName: string | null = null;
  currentProjectFileHandle: FileSystemFileHandle | null = null;
  isEditorActive: boolean = false;
  projectLoadingStatus: "idle" | "loading" | "success" | "error" = "idle";
  projectLoadingMessage: string = "";
  projectLoadingProgress: number = 0;

  constructor(private root: RootStore) {
    makeAutoObservable(this);
  }

  setCurrentProjectFilePath(filePath: string | null) {
    this.currentProjectFilePath = filePath;
  }

  setCurrentProjectFileName(fileName: string | null) {
    this.currentProjectFileName = fileName;
  }

  setCurrentProjectFileHandle(fileHandle: FileSystemFileHandle | null) {
    this.currentProjectFileHandle = fileHandle;
  }

  setEditorActive(active: boolean) {
    this.isEditorActive = active;
  }

  setProjectLoadingStatus(
    status: "idle" | "loading" | "success" | "error",
    message?: string
  ) {
    this.projectLoadingStatus = status;
    if (message !== undefined) this.projectLoadingMessage = message;
  }

  setProjectLoadingProgress(progress: number) {
    this.projectLoadingProgress = progress;
  }

  async serialize(): Promise<ArrayBuffer> {
    const mediaPromises: Promise<{ id: string; data: string; type: string }>[] = [];

    for (const element of this.root.elementStore.editorElements) {
      if (["image", "video", "audio"].includes(element.type)) {
        if ("src" in element.properties) {
          const src = (element.properties as any).src as string;
          if (typeof src === "string" && (src.startsWith("blob:") || src.startsWith("data:"))) {
            mediaPromises.push(
              fetch(src)
                .then((response) => response.blob())
                .then(
                  (blob) =>
                    new Promise<{ id: string; data: string; type: string }>((resolve) => {
                      const reader = new FileReader();
                      reader.onloadend = () =>
                        resolve({
                          id: element.id,
                          data: reader.result as string,
                          type: element.type,
                        });
                      reader.readAsDataURL(blob);
                    })
                )
            );
          }
        }
      }
    }

    const mediaFiles = await Promise.all(mediaPromises);

    const editorElementsForSerialization = this.root.elementStore.editorElements.map((element) => {
      if (element.type === "audio") {
        const audioEl = element as AudioEditorElement;
        return {
          ...audioEl,
          konvaNode: undefined,
          properties: {
            ...audioEl.properties,
            volume: audioEl.properties.volume !== undefined ? audioEl.properties.volume : 1,
            muted: audioEl.properties.muted !== undefined ? audioEl.properties.muted : false,
            masterVolume: audioEl.properties.masterVolume,
          },
        };
      }
      return { ...element, konvaNode: undefined };
    });

    const stateObject = {
      backgroundColor: this.root.canvasStore.backgroundColor,
      selectedMenuOption: this.root.selectedMenuOption,
      audios: this.root.mediaStore.audios,
      videos: this.root.mediaStore.videos,
      images: this.root.mediaStore.images,
      editorElements: editorElementsForSerialization,
      maxTime: this.root.playbackStore.maxTime,
      animations: this.root.animationStore.animations,
      currentKeyFrame: this.root.playbackStore.currentKeyFrame,
      fps: this.root.playbackStore.fps,
      selectedVideoFormat: this.root.exportStore.selectedVideoFormat,
      canvas_width: this.root.canvasStore.canvas_width,
      canvas_height: this.root.canvasStore.canvas_height,
      mediaFiles,
    };

    const stateJSON = JSON.stringify(stateObject);
    const encoder = new TextEncoder();
    const stateBuffer = encoder.encode(stateJSON);

    try {
      const compressed = pako.deflate(stateBuffer, { level: 9 });
      return compressed.buffer.slice(0) as ArrayBuffer;
    } catch (error) {
      console.error("Compression failed, returning uncompressed data:", error);
      const arrayBuffer = new ArrayBuffer(stateBuffer.byteLength);
      const view = new Uint8Array(arrayBuffer);
      view.set(stateBuffer);
      return arrayBuffer;
    }
  }

  deserialize(projectState: ArrayBuffer): void {
    let stateJSON: string;

    try {
      const decompressed = pako.inflate(new Uint8Array(projectState));
      const decoder = new TextDecoder();
      stateJSON = decoder.decode(decompressed);
    } catch (error) {
      console.warn("Decompression failed, trying to parse as uncompressed data:", error);
      const decoder = new TextDecoder();
      stateJSON = decoder.decode(projectState);
    }

    const stateObject = JSON.parse(stateJSON);

    this.root.elementStore.selectedElement = null;
    this.root.canvasStore.backgroundColor = stateObject.backgroundColor;
    this.root.selectedMenuOption = stateObject.selectedMenuOption;

    this.root.mediaStore.audios = [];
    this.root.mediaStore.videos = [];
    this.root.mediaStore.images = [];

    this.root.elementStore.editorElements = stateObject.editorElements.map((element: any) => {
      if (element.type === "audio") {
        return {
          ...element,
          properties: {
            ...element.properties,
            volume: element.properties.volume !== undefined ? element.properties.volume : 1,
            muted: element.properties.muted !== undefined ? element.properties.muted : false,
          },
        };
      }
      return element;
    });

    this.root.playbackStore.maxTime = stateObject.maxTime;
    this.root.animationStore.animations = stateObject.animations;
    this.root.playbackStore.currentKeyFrame = stateObject.currentKeyFrame;
    this.root.playbackStore.fps = stateObject.fps;
    this.root.exportStore.selectedVideoFormat = stateObject.selectedVideoFormat;
    this.root.canvasStore.canvas_width = stateObject.canvas_width;
    this.root.canvasStore.canvas_height = stateObject.canvas_height;

    const mediaDataMap = new Map<string, string>();
    const validImages = new Set<string>();
    const validVideos = new Set<string>();
    const validAudios = new Set<string>();

    if (stateObject.mediaFiles && Array.isArray(stateObject.mediaFiles)) {
      for (const mediaFile of stateObject.mediaFiles) {
        mediaDataMap.set(mediaFile.id, mediaFile.data);
        if (mediaFile.type === "image") {
          this.root.mediaStore.images.push(mediaFile.data);
          validImages.add(mediaFile.data);
        } else if (mediaFile.type === "video") {
          this.root.mediaStore.videos.push(mediaFile.data);
          validVideos.add(mediaFile.data);
        } else if (mediaFile.type === "audio") {
          this.root.mediaStore.audios.push(mediaFile.data);
          validAudios.add(mediaFile.data);
        }
      }
    }

    for (const element of this.root.elementStore.editorElements) {
      if (["image", "video", "audio"].includes(element.type)) {
        if ("properties" in element && "src" in (element.properties as any)) {
          const dataUrl = mediaDataMap.get(element.id);
          if (dataUrl) {
            (element.properties as any).src = dataUrl;
            if (element.type === "image") validImages.add(dataUrl);
            if (element.type === "video") validVideos.add(dataUrl);
            if (element.type === "audio") validAudios.add(dataUrl);
          } else if (typeof (element.properties as any).src === "string") {
            const src = (element.properties as any).src as string;
            if (element.type === "image") validImages.add(src);
            if (element.type === "video") validVideos.add(src);
            if (element.type === "audio") validAudios.add(src);
          }
        }
      }
    }

    const isValidUrl = (url: string) =>
      url.startsWith("data:") || !url.startsWith("blob:");

    if (Array.isArray(stateObject.audios)) {
      stateObject.audios.forEach((audio: string) => {
        if (isValidUrl(audio) && !this.root.mediaStore.audios.includes(audio)) {
          this.root.mediaStore.audios.push(audio);
          validAudios.add(audio);
        }
      });
    }
    if (Array.isArray(stateObject.videos)) {
      stateObject.videos.forEach((video: string) => {
        if (isValidUrl(video) && !this.root.mediaStore.videos.includes(video)) {
          this.root.mediaStore.videos.push(video);
          validVideos.add(video);
        }
      });
    }
    if (Array.isArray(stateObject.images)) {
      stateObject.images.forEach((image: string) => {
        if (isValidUrl(image) && !this.root.mediaStore.images.includes(image)) {
          this.root.mediaStore.images.push(image);
          validImages.add(image);
        }
      });
    }

    this.root.mediaStore.images = [...new Set(this.root.mediaStore.images.filter((i) => validImages.has(i)))];
    this.root.mediaStore.videos = [...new Set(this.root.mediaStore.videos.filter((v) => validVideos.has(v)))];
    this.root.mediaStore.audios = [...new Set(this.root.mediaStore.audios.filter((a) => validAudios.has(a)))];

    this.root.animationStore.animationTimeLine = createTimeline({ autoplay: false });
    this.root.elementStore.refreshElements();
    this.root.animationStore.refreshAnimations();
  }
}
