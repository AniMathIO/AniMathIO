"use client";
import { makeAutoObservable } from "mobx";
import Konva from "konva";
import { CanvasStore } from "./stores/CanvasStore";
import { MediaStore } from "./stores/MediaStore";
import { ElementStore } from "./stores/ElementStore";
import { AnimationStore } from "./stores/AnimationStore";
import { PlaybackStore } from "./stores/PlaybackStore";
import { ExportStore } from "./stores/ExportStore";
import { ProjectStore } from "./stores/ProjectStore";
import { UIStore } from "./stores/UIStore";
import { AudioContextStore } from "./stores/AudioContextStore";
import { KeyboardShortcutService } from "./stores/KeyboardShortcutService";
import {
  EditorElement,
  Animation,
  TimeFrame,
  MenuOption,
  Effect,
} from "@/types";

export class RootStore {
  // Sub-stores (use *Store suffix to avoid conflicts with forwarding getters)
  readonly canvasStore: CanvasStore;
  readonly mediaStore: MediaStore;
  readonly elementStore: ElementStore;
  readonly animationStore: AnimationStore;
  readonly playbackStore: PlaybackStore;
  readonly exportStore: ExportStore;
  readonly projectStore: ProjectStore;
  readonly uiStore: UIStore;
  readonly audioContextStore: AudioContextStore;
  readonly keyboardShortcutService: KeyboardShortcutService;

  constructor() {
    this.canvasStore = new CanvasStore(this);
    this.mediaStore = new MediaStore(this);
    this.elementStore = new ElementStore(this);
    this.animationStore = new AnimationStore(this);
    this.playbackStore = new PlaybackStore(this);
    this.exportStore = new ExportStore(this);
    this.projectStore = new ProjectStore(this);
    this.uiStore = new UIStore(this);
    this.audioContextStore = new AudioContextStore(this);
    this.keyboardShortcutService = new KeyboardShortcutService(this);

    makeAutoObservable(this, {
      canvasStore: false,
      mediaStore: false,
      elementStore: false,
      animationStore: false,
      playbackStore: false,
      exportStore: false,
      projectStore: false,
      uiStore: false,
      audioContextStore: false,
      keyboardShortcutService: false,
    });

    if (typeof window !== "undefined") {
      window.addEventListener(
        "keydown",
        this.keyboardShortcutService.handleKeyboardShortcut
      );
    }
  }

  // ---------- AudioContext management ----------

  get audioContexts() { return this.audioContextStore.audioContexts; }
  getAudioContext(audioElement: HTMLAudioElement) {
    return this.audioContextStore.getAudioContext(audioElement);
  }

  /**
   * Remove and close the cached AudioContext (if any) bound to the given
   * element id. Called when an editor element is deleted so a later element
   * that reuses the same id doesn't inherit a source node bound to a
   * detached DOM node, and so we don't leak AudioContexts over time.
   */
  releaseAudioContext(elementId: string) {
    this.audioContextStore.releaseAudioContext(elementId);
  }

  /**
   * Close and clear every cached AudioContext. Called on project switch so
   * contexts from the previous project don't stay open for the rest of the
   * session (see AudioContextStore.releaseAll for why this matters).
   */
  releaseAllAudioContexts() {
    this.audioContextStore.releaseAll();
  }

  // ---------- Keyboard shortcuts ----------

  handleKeyboardShortcut(event: KeyboardEvent) {
    this.keyboardShortcutService.handleKeyboardShortcut(event);
  }

  // ============================================================
  // Backward-compatibility forwarding getters/setters
  // These preserve the flat API that all existing components use.
  // ============================================================

  // --- Canvas ---
  get stage() { return this.canvasStore.stage; }
  get layer() { return this.canvasStore.layer; }
  get canvas_width() { return this.canvasStore.canvas_width; }
  get canvas_height() { return this.canvasStore.canvas_height; }
  get backgroundColor() { return this.canvasStore.backgroundColor; }
  setStage(stage: Konva.Stage | null, layer: Konva.Layer | null, w: number, h: number) {
    this.canvasStore.setStage(stage, layer, w, h);
  }
  setCanvasSize(w: number, h: number) { this.canvasStore.setCanvasSize(w, h); }
  setBackgroundColor(color: string) { this.canvasStore.setBackgroundColor(color); }

  // --- Media ---
  get videos() { return this.mediaStore.videos; }
  get images() { return this.mediaStore.images; }
  get audios() { return this.mediaStore.audios; }
  setVideos(v: string[]) { this.mediaStore.setVideos(v); }
  setImages(i: string[]) { this.mediaStore.setImages(i); }
  setAudios(a: string[]) { this.mediaStore.setAudios(a); }
  addVideoResource(v: string) { this.mediaStore.addVideoResource(v); }
  addAudioResource(a: string) { this.mediaStore.addAudioResource(a); }
  addImageResource(i: string) { this.mediaStore.addImageResource(i); }
  replaceImageResource(index: number, url: string) { this.mediaStore.replaceImageResource(index, url); }

  // --- Elements ---
  get editorElements() { return this.elementStore.editorElements; }
  get selectedElement() { return this.elementStore.selectedElement; }
  get clipboard() { return this.elementStore.clipboard; }
  set clipboard(val: EditorElement | null) { this.elementStore.clipboard = val; }
  get konvaNodes() { return this.elementStore.konvaNodes; }
  setEditorElements(els: EditorElement[]) { this.elementStore.setEditorElements(els); }
  async updateEditorElement(el: EditorElement) { return this.elementStore.updateEditorElement(el); }
  async updateEditorElementTimeFrame(el: EditorElement, tf: Partial<TimeFrame>) {
    return this.elementStore.updateEditorElementTimeFrame(el, tf);
  }
  addEditorElement(el: EditorElement) { return this.elementStore.addEditorElement(el); }
  removeEditorElement(id: string) { this.elementStore.removeEditorElement(id); }
  setSelectedElement(el: EditorElement | null) { this.elementStore.setSelectedElement(el); }
  updateSelectedElement() { this.elementStore.updateSelectedElement(); }
  refreshElements() { this.elementStore.refreshElements(); }
  updateEffect(id: string, effect: Effect) { this.elementStore.updateEffect(id, effect as any); }
  addVideo(index: number, dropPosition?: { x: number; y: number }) { this.elementStore.addVideo(index, dropPosition); }
  addImage(index: number, dropPosition?: { x: number; y: number }) { this.elementStore.addImage(index, dropPosition); }
  addMafsResource(index: number, pngSrc: string, name: string) {
    return this.elementStore.addMafsResource(index, pngSrc, name);
  }
  addAudio(index: number) { this.elementStore.addAudio(index); }
  addText(options: { text: string; fontSize: number; fontWeight: number }, dropPosition?: { x: number; y: number }) {
    this.elementStore.addText(options, dropPosition);
  }
  updateAudioSettings(
    id: string,
    settings: Partial<{ volume: number; muted: boolean; masterVolume?: number }>
  ) { this.elementStore.updateAudioSettings(id, settings); }
  setKonvaNode(id: string, node: Konva.Node | null) { this.elementStore.setKonvaNode(id, node); }
  getKonvaNode(id: string) { return this.elementStore.getKonvaNode(id); }
  deleteSelectedObjects(els: EditorElement[] | null) { this.elementStore.deleteSelectedObjects(els); }
  copyObject() { this.elementStore.copyObject(); }
  pasteObject() { this.elementStore.pasteObject(); }
  moveSelectedObject(direction: string) { this.elementStore.moveSelectedObject(direction); }

  // --- Animations (array, not AnimationStore) ---
  get animations() { return this.animationStore.animations; }
  get animationTimeLine() { return this.animationStore.animationTimeLine; }
  addAnimation(a: Animation) { this.animationStore.addAnimation(a); }
  updateAnimation(id: string, a: Animation) { this.animationStore.updateAnimation(id, a); }
  removeAnimation(id: string) { this.animationStore.removeAnimation(id); }
  setAnimations(a: Animation[]) { this.animationStore.setAnimations(a); }
  refreshAnimations() { this.animationStore.refreshAnimations(); }

  // --- Playback ---
  get playing() { return this.playbackStore.playing; }
  get currentKeyFrame() { return this.playbackStore.currentKeyFrame; }
  get fps() { return this.playbackStore.fps; }
  get maxTime() { return this.playbackStore.maxTime; }
  get currentTimeInMs() { return this.playbackStore.currentTimeInMs; }
  // playback is also exposed as a sub-store for direct access in ExportStore etc.
  get playback() { return this.playbackStore; }
  setMaxTime(t: number) { this.playbackStore.setMaxTime(t); }
  setCurrentKeyFrame(k: number) { this.playbackStore.setCurrentKeyFrame(k); }
  setPlaying(p: boolean) { this.playbackStore.setPlaying(p); }
  playFrames() { this.playbackStore.playFrames(); }
  updateTimeTo(t: number) { this.playbackStore.updateTimeTo(t); }
  handleSeek(s: number) { this.playbackStore.handleSeek(s); }
  skipForward() { this.playbackStore.skipForward(); }
  skipBackward() { this.playbackStore.skipBackward(); }
  skipToStart() { this.playbackStore.skipToStart(); }
  skipToEnd() { this.playbackStore.skipToEnd(); }
  skipInTime(key: string) { this.playbackStore.skipInTime(key); }
  updateVideoElements() { this.playbackStore.updateVideoElements(); }
  updateAudioElements() { this.playbackStore.updateAudioElements(); }

  // --- Export ---
  get possibleVideoFormats() { return this.exportStore.possibleVideoFormats; }
  get selectedVideoFormat() { return this.exportStore.selectedVideoFormat; }
  setVideoFormat(f: "mp4" | "webm") { this.exportStore.setVideoFormat(f); }
  saveCanvasToVideoWithAudio() { this.exportStore.saveCanvasToVideoWithAudio(); }

  // --- Project ---
  get currentProjectFilePath() { return this.projectStore.currentProjectFilePath; }
  get currentProjectFileName() { return this.projectStore.currentProjectFileName; }
  get currentProjectFileHandle() { return this.projectStore.currentProjectFileHandle; }
  get isEditorActive() { return this.projectStore.isEditorActive; }
  get projectLoadingStatus() { return this.projectStore.projectLoadingStatus; }
  get projectLoadingMessage() { return this.projectStore.projectLoadingMessage; }
  get projectLoadingProgress() { return this.projectStore.projectLoadingProgress; }
  setCurrentProjectFilePath(p: string | null) { this.projectStore.setCurrentProjectFilePath(p); }
  setCurrentProjectFileName(n: string | null) { this.projectStore.setCurrentProjectFileName(n); }
  setCurrentProjectFileHandle(h: FileSystemFileHandle | null) {
    this.projectStore.setCurrentProjectFileHandle(h);
  }
  setEditorActive(active: boolean) { this.projectStore.setEditorActive(active); }
  setProjectLoadingStatus(status: "idle" | "loading" | "success" | "error", message?: string) {
    this.projectStore.setProjectLoadingStatus(status, message);
  }
  setProjectLoadingProgress(p: number) { this.projectStore.setProjectLoadingProgress(p); }
  serialize() { return this.projectStore.serialize(); }
  deserialize(data: ArrayBuffer) { this.projectStore.deserialize(data); }

  // --- Selected menu ---
  get selectedMenuOption() { return this.uiStore.selectedMenuOption; }
  set selectedMenuOption(option: MenuOption) { this.uiStore.selectedMenuOption = option; }
  setSelectedMenuOption(option: MenuOption) {
    this.uiStore.setSelectedMenuOption(option);
  }

  /**
   * Reset editor state for a brand-new project (MobX-safe: do not assign to getters like editorElements).
   */
  resetForNewProject() {
    this.playbackStore.setPlaying(false);
    this.elementStore.konvaNodes.clear();
    this.elementStore.clipboard = null;
    this.releaseAllAudioContexts();
    this.setAnimations([]);
    this.setEditorElements([]);
    this.setVideos([]);
    this.setImages([]);
    this.setAudios([]);
    this.setBackgroundColor("#111111");
    this.setMaxTime(30 * 1000);
    this.setCanvasSize(800, 600);
    this.setCurrentKeyFrame(0);
    this.setSelectedElement(null);
  }
}
