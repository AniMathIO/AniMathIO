import { makeAutoObservable } from "mobx";
import Konva from "konva";
import type { RootStore } from "../RootStore";
import {
  EditorElement,
  TimeFrame,
  VideoEditorElement,
  AudioEditorElement,
  TextEditorElement,
  Placement,
} from "@/types";
import { getUid, isHtmlVideoElement, isHtmlAudioElement, isHtmlImageElement } from "@/utils";

export class ElementStore {
  editorElements: EditorElement[] = [];
  selectedElement: EditorElement | null = null;
  clipboard: EditorElement | null = null;

  // Non-observable: Konva node registry (keyed by element id)
  konvaNodes: Map<string, Konva.Node> = new Map();

  constructor(private root: RootStore) {
    makeAutoObservable(this, {
      konvaNodes: false,
    });
  }

  // ---------- Konva node registration ----------

  setKonvaNode(id: string, node: Konva.Node | null) {
    if (node) {
      this.konvaNodes.set(id, node);
      // A node registers itself asynchronously (on mount), after any
      // refreshElements()/updateTimeTo() call already ran for this change.
      // Apply the current timeframe visibility immediately so the element
      // doesn't default to Konva's visible:true until the next seek/play tick.
      const element = this.editorElements.find((el) => el.id === id);
      if (element) {
        const t = this.root.playbackStore.currentTimeInMs;
        node.visible(element.timeFrame.start <= t && t <= element.timeFrame.end);
      }
    } else {
      this.konvaNodes.delete(id);
    }
  }

  getKonvaNode(id: string): Konva.Node | undefined {
    return this.konvaNodes.get(id);
  }

  // ---------- Element management ----------

  setEditorElements(editorElements: EditorElement[]) {
    this.editorElements = editorElements;
    this.updateSelectedElement();
    this.refreshElements();
    this.root.animationStore.refreshAnimations();
  }

  async updateEditorElement(editorElement: EditorElement): Promise<void> {
    await this.setEditorElements(
      this.editorElements.map((element) =>
        element.id === editorElement.id ? editorElement : element
      )
    );
    this.updateSelectedElement();
  }

  async updateEditorElementTimeFrame(
    editorElement: EditorElement,
    timeFrame: Partial<TimeFrame>
  ): Promise<void> {
    if (timeFrame.start === undefined && timeFrame.end === undefined) return;
    if (timeFrame.start !== undefined && timeFrame.start < 0) timeFrame.start = 0;
    if (timeFrame.end !== undefined && timeFrame.end > this.root.playbackStore.maxTime) {
      timeFrame.end = this.root.playbackStore.maxTime;
    }

    const newEditorElement = {
      ...editorElement,
      timeFrame: { ...editorElement.timeFrame, ...timeFrame },
    };

    this.root.playbackStore.updateVideoElements();
    this.root.playbackStore.updateAudioElements();
    await this.updateEditorElement(newEditorElement);
    this.root.animationStore.refreshAnimations();
  }

  addEditorElement(editorElement: EditorElement) {
    return new Promise<void>((resolve) => {
      this.setEditorElements([...this.editorElements, editorElement]);
      this.refreshElements();
      this.setSelectedElement(this.editorElements[this.editorElements.length - 1]);
      resolve();
    });
  }

  removeEditorElement(id: string) {
    this.konvaNodes.delete(id);
    this.setEditorElements(
      this.editorElements.filter((editorElement) => editorElement.id !== id)
    );
    this.refreshElements();
  }

  updateSelectedElement() {
    this.selectedElement =
      this.editorElements.find((element) => element.id === this.selectedElement?.id) ?? null;
  }

  setSelectedElement(selectedElement: EditorElement | null) {
    this.selectedElement = selectedElement;
  }

  /**
   * refreshElements: In the Konva+React architecture, React declaratively renders
   * elements from editorElements. This method just triggers downstream refreshes.
   */
  refreshElements() {
    this.root.animationStore.refreshAnimations();
    this.root.playbackStore.updateTimeTo(this.root.playbackStore.currentTimeInMs);
    this.root.canvasStore.layer?.batchDraw();
  }

  // ---------- Add element helpers ----------

  addVideo(index: number, dropPosition?: { x: number; y: number }) {
    const videoElement = document.getElementById(`video-${index}`);
    if (!isHtmlVideoElement(videoElement)) return;

    const videoDurationMs = videoElement.duration * 1000;
    const aspectRatio = videoElement.videoWidth / videoElement.videoHeight;
    const videoId = getUid();
    const audioId = getUid();
    const width = 100 * aspectRatio;
    const height = 100;
    const x = dropPosition ? dropPosition.x - width / 2 : 0;
    const y = dropPosition ? dropPosition.y - height / 2 : 0;

    this.addEditorElement({
      id: videoId,
      name: `Media(video) ${index + 1}`,
      type: "video",
      placement: {
        x,
        y,
        width,
        height,
        rotation: 0,
        scaleX: 1,
        scaleY: 1,
      },
      timeFrame: { start: 0, end: videoDurationMs },
      properties: {
        elementId: `video-${videoId}`,
        src: videoElement.src,
        effect: { type: "none" },
        muted: true,
      },
    });

    this.addEditorElement({
      id: audioId,
      name: `Media(audio) ${index + 1}`,
      type: "audio",
      placement: {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
        rotation: 0,
        scaleX: 1,
        scaleY: 1,
      },
      timeFrame: { start: 0, end: videoDurationMs },
      properties: {
        elementId: `audio-${audioId}`,
        src: videoElement.src,
        volume: 1,
        muted: false,
      },
    });
  }

  addImage(index: number, dropPosition?: { x: number; y: number }) {
    const imageElement = document.getElementById(`image-${index}`);
    if (!isHtmlImageElement(imageElement)) return;

    const aspectRatio = imageElement.naturalWidth / imageElement.naturalHeight;
    const id = getUid();
    const width = 100 * aspectRatio;
    const height = 100;
    const x = dropPosition ? dropPosition.x - width / 2 : 0;
    const y = dropPosition ? dropPosition.y - height / 2 : 0;

    this.addEditorElement({
      id,
      name: `Media(image) ${index + 1}`,
      type: "image",
      placement: {
        x,
        y,
        width,
        height,
        rotation: 0,
        scaleX: 1,
        scaleY: 1,
      },
      timeFrame: { start: 0, end: this.root.playbackStore.maxTime },
      properties: {
        elementId: `image-${id}`,
        src: imageElement.src,
        effect: { type: "none" },
      },
    });
  }

  addMafsResource(index: number, pngSrc: string, name: string) {
    return new Promise<void>((resolve, reject) => {
      const img = new Image();
      img.crossOrigin = "anonymous";
      img.onload = () => {
        const id = getUid();
        this.addEditorElement({
          id,
          name: `Mafs(${name}) ${index + 1}`,
          type: "mafs",
          placement: {
            x: 0,
            y: 0,
            width: img.width || 100,
            height: img.height || 100,
            rotation: 0,
            scaleX: 1,
            scaleY: 1,
          },
          timeFrame: { start: 0, end: this.root.playbackStore.maxTime },
          properties: {
            elementId: `mafs-${id}`,
            src: pngSrc,
            effect: { type: "none" },
          },
        });
        resolve();
      };
      img.onerror = reject;
      img.src = pngSrc;
    });
  }

  addAudio(index: number) {
    const audioElement = document.getElementById(`audio-${index}`);
    if (!isHtmlAudioElement(audioElement)) return;

    const audioDurationMs = audioElement.duration * 1000;
    const id = getUid();

    this.addEditorElement({
      id,
      name: `Media(audio) ${index + 1}`,
      type: "audio",
      placement: {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
        rotation: 0,
        scaleX: 1,
        scaleY: 1,
      },
      timeFrame: { start: 0, end: audioDurationMs },
      properties: {
        elementId: `audio-${id}`,
        src: audioElement.src,
        volume: 1,
        muted: false,
      },
    });
  }

  addText(options: { text: string; fontSize: number; fontWeight: number }, dropPosition?: { x: number; y: number }) {
    const id = getUid();
    const index = this.editorElements.length;
    const width = 100;
    const height = 100;
    const x = dropPosition ? dropPosition.x - width / 2 : 0;
    const y = dropPosition ? dropPosition.y - height / 2 : 0;
    this.addEditorElement({
      id,
      name: `Text ${index + 1}`,
      type: "text",
      placement: {
        x,
        y,
        width,
        height,
        rotation: 0,
        scaleX: 1,
        scaleY: 1,
      },
      timeFrame: { start: 0, end: this.root.playbackStore.maxTime },
      properties: {
        text: options.text,
        fontSize: options.fontSize,
        fontWeight: options.fontWeight,
        splittedTexts: [],
      },
    });
  }

  updateAudioSettings(
    id: string,
    settings: Partial<{ volume: number; muted: boolean; masterVolume?: number }>
  ) {
    const index = this.editorElements.findIndex((element) => element.id === id);
    if (index >= 0 && this.editorElements[index].type === "audio") {
      const element = this.editorElements[index] as AudioEditorElement;
      if (settings.volume !== undefined) element.properties.volume = settings.volume;
      if (settings.muted !== undefined) element.properties.muted = settings.muted;
      if (settings.masterVolume !== undefined) element.properties.masterVolume = settings.masterVolume;
    }
  }

  updateEffect(id: string, effect: { type: string }) {
    const index = this.editorElements.findIndex((element) => element.id === id);
    const element = this.editorElements[index];
    if (element && (element.type === "video" || element.type === "image" || element.type === "mafs")) {
      (element.properties as any).effect = effect;
    }
    this.refreshElements();
  }

  // ---------- Copy / paste / delete ----------

  deleteSelectedObjects(selectedObjects: EditorElement[] | null) {
    selectedObjects?.forEach((selectedObject) => {
      if (selectedObject) this.removeEditorElement(selectedObject.id);
    });
  }

  copyObject() {
    if (this.selectedElement) {
      this.clipboard = this.selectedElement;
    }
  }

  pasteObject() {
    if (!this.clipboard) return;
    const type = this.clipboard.type;
    switch (type) {
      case "text": {
        const { text, fontSize, fontWeight } = (this.clipboard as TextEditorElement).properties;
        this.addText({ text, fontSize, fontWeight });
        break;
      }
      case "image": {
        const imageIndex = parseInt(
          this.clipboard.name.match(/(\d+)\s*$/)?.toString() || "",
          10
        );
        this.addImage(imageIndex - 1);
        break;
      }
      case "mafs": {
        const mafsIndex = parseInt(
          this.clipboard.name.match(/(\d+)\s*$/)?.toString() || "",
          10
        );
        const mafsName = this.clipboard.name.match(/\(([^)]+)\)/)?.[1] || "";
        const mafsPngSrc = (this.clipboard.properties as any).src;
        this.addMafsResource(mafsIndex - 1, mafsPngSrc, mafsName);
        break;
      }
      case "video": {
        const videoIndex = parseInt(
          this.clipboard.name.match(/(\d+)\s*$/)?.toString() || "",
          10
        );
        this.addVideo(videoIndex - 1);
        break;
      }
      default:
        break;
    }
  }

  moveSelectedObject(direction: string) {
    if (!this.selectedElement) return;
    const delta = 10;
    const { placement } = this.selectedElement;
    let newPlacement: Placement = { ...placement };

    if (direction === "ArrowUp") newPlacement = { ...newPlacement, y: placement.y - delta };
    if (direction === "ArrowDown") newPlacement = { ...newPlacement, y: placement.y + delta };
    if (direction === "ArrowLeft") newPlacement = { ...newPlacement, x: placement.x - delta };
    if (direction === "ArrowRight") newPlacement = { ...newPlacement, x: placement.x + delta };

    this.updateEditorElement({ ...this.selectedElement, placement: newPlacement });

    // Also update the Konva node directly for smooth movement
    const node = this.konvaNodes.get(this.selectedElement.id);
    if (node) {
      node.x(newPlacement.x);
      node.y(newPlacement.y);
      this.root.canvasStore.layer?.batchDraw();
    }
  }
}
