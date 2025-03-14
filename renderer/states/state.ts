import { makeAutoObservable } from "mobx";
import { fabric } from "fabric";
import {
  getUid,
  isHtmlAudioElement,
  isHtmlImageElement,
  isHtmlVideoElement,
} from "@/utils";
import anime, { get } from "animejs";
import {
  MenuOption,
  EditorElement,
  Animation,
  TimeFrame,
  VideoEditorElement,
  AudioEditorElement,
  Placement,
  ImageEditorElement,
  Effect,
  TextEditorElement,
  MafsEditorElement,
} from "@/types";
import { FabricUtils } from "../utils/fabric-utils";
import { FFmpeg } from "@ffmpeg/ffmpeg";
import { toBlobURL } from "@ffmpeg/util";
import fixWebmDuration from "webm-duration-fix";
import { AlignGuidelines } from "fabric-guideline-plugin";
import * as pako from "pako";
export class State {
  canvas: fabric.Canvas | null;
  canvasBox: fabric.Rect | null = null; // helper rect for guidelines
  backgroundColor: string;

  clipboard: EditorElement | null = null; // For storing copied objects temporarily

  selectedMenuOption: MenuOption;
  audios: string[];
  videos: string[];
  images: string[];
  editorElements: EditorElement[];
  selectedElement: EditorElement | null;

  maxTime: number;
  animations: Animation[];
  animationTimeLine: anime.AnimeTimelineInstance | null = null;
  playing: boolean;

  currentKeyFrame: number;
  fps: number;

  possibleVideoFormats: string[] = ["mp4", "webm"];
  selectedVideoFormat: "mp4" | "webm";

  audioContexts = new Map<
    string,
    { context: AudioContext; sourceNode: MediaElementAudioSourceNode }
  >();
  canvas_width: number;
  canvas_height: number;

  constructor() {
    this.canvas = null;
    this.videos = [];
    this.images = [];
    this.audios = [];
    this.editorElements = [];
    this.backgroundColor = "#111111";
    this.maxTime = 30 * 1000;
    this.playing = false;
    this.currentKeyFrame = 0;
    this.selectedElement = null;
    this.fps = 60;
    this.animations = [];
    this.canvas_width = 800;
    this.canvas_height = 600;
    if (typeof window !== "undefined") {
      // Code that uses anime.js
      this.animationTimeLine = anime.timeline({ autoplay: false });
    }
    this.selectedMenuOption = "Videos";
    this.selectedVideoFormat = "mp4";
    makeAutoObservable(this);

    if (typeof window !== "undefined") {
      window.addEventListener(
        "keydown",
        this.handleKeyboardShortcut.bind(this)
      );
    }
  }

  async serialize(): Promise<ArrayBuffer> {
    // Create an array to store all media file promises
    const mediaPromises: Promise<{ id: string; data: string; type: string }>[] =
      [];

    // For each media element in your editor, convert to base64
    for (const element of this.editorElements) {
      // Type narrowing to check if element is a media type
      if (["image", "video", "audio"].includes(element.type)) {
        // Type guard to check if properties has a src property
        if ("src" in element.properties) {
          const src = element.properties.src;
          if (
            typeof src === "string" &&
            (src.startsWith("blob:") || src.startsWith("data:"))
          ) {
            // For blob URLs or data URLs, we need to fetch and encode
            mediaPromises.push(
              fetch(src)
                .then((response) => response.blob())
                .then(
                  (blob) =>
                    new Promise<{ id: string; data: string; type: string }>(
                      (resolve) => {
                        const reader = new FileReader();
                        reader.onloadend = () =>
                          resolve({
                            id: element.id,
                            data: reader.result as string,
                            type: element.type,
                          });
                        reader.readAsDataURL(blob);
                      }
                    )
                )
            );
          }
        }
      }
    }

    // Rest of the code remains the same
    const mediaFiles = await Promise.all(mediaPromises);

    const stateObject = {
      backgroundColor: this.backgroundColor,
      selectedMenuOption: this.selectedMenuOption,
      audios: this.audios,
      videos: this.videos,
      images: this.images,
      editorElements: this.editorElements,
      maxTime: this.maxTime,
      animations: this.animations,
      currentKeyFrame: this.currentKeyFrame,
      fps: this.fps,
      selectedVideoFormat: this.selectedVideoFormat,
      canvas_width: this.canvas_width,
      canvas_height: this.canvas_height,
      mediaFiles: mediaFiles, // Add media files to the state
    };

    const stateJSON = JSON.stringify(stateObject);
    const encoder = new TextEncoder();
    const stateBuffer = encoder.encode(stateJSON);

    // Compress the data using pako
    try {
      // Use higher compression level (9 is maximum)
      const compressed = pako.deflate(stateBuffer, { level: 9 });
      // Convert ArrayBufferLike to ArrayBuffer explicitly
      const compressedArrayBuffer = compressed.buffer.slice(0);
      return compressedArrayBuffer as ArrayBuffer;
    } catch (error) {
      console.error("Compression failed, returning uncompressed data:", error);
      // Return uncompressed data as fallback
      const arrayBuffer = new ArrayBuffer(stateBuffer.byteLength);
      const view = new Uint8Array(arrayBuffer);
      view.set(stateBuffer);
      return arrayBuffer;
    }
  }

  deserialize(projectState: ArrayBuffer): void {
    let stateJSON: string;

    try {
      // Try to decompress using pako
      const decompressed = pako.inflate(new Uint8Array(projectState));
      const decoder = new TextDecoder();
      stateJSON = decoder.decode(decompressed);
    } catch (error) {
      // If decompression fails, assume it's an uncompressed file (for backward compatibility)
      console.warn(
        "Decompression failed, trying to parse as uncompressed data:",
        error
      );
      const decoder = new TextDecoder();
      stateJSON = decoder.decode(projectState);
    }

    // Parse the JSON
    const stateObject = JSON.parse(stateJSON);

    // First load basic state properties
    this.backgroundColor = stateObject.backgroundColor;
    this.selectedMenuOption = stateObject.selectedMenuOption;

    // Create new arrays for media resources
    this.audios = [];
    this.videos = [];
    this.images = [];

    this.editorElements = stateObject.editorElements;
    this.maxTime = stateObject.maxTime;
    this.animations = stateObject.animations;
    this.currentKeyFrame = stateObject.currentKeyFrame;
    this.fps = stateObject.fps;
    this.selectedVideoFormat = stateObject.selectedVideoFormat;
    this.canvas_width = stateObject.canvas_width;
    this.canvas_height = stateObject.canvas_height;

    // Create a map to store data URLs by element ID
    const mediaDataMap = new Map<string, string>();

    // Track valid resources to clean up any invalid ones later
    const validImages = new Set<string>();
    const validVideos = new Set<string>();
    const validAudios = new Set<string>();

    // If we have saved media files, process them
    if (stateObject.mediaFiles && Array.isArray(stateObject.mediaFiles)) {
      for (const mediaFile of stateObject.mediaFiles) {
        // Store data URLs in our map
        mediaDataMap.set(mediaFile.id, mediaFile.data);

        // Add to the appropriate resource array based on type
        if (mediaFile.type === "image") {
          this.images.push(mediaFile.data);
          validImages.add(mediaFile.data);
        } else if (mediaFile.type === "video") {
          this.videos.push(mediaFile.data);
          validVideos.add(mediaFile.data);
        } else if (mediaFile.type === "audio") {
          this.audios.push(mediaFile.data);
          validAudios.add(mediaFile.data);
        }
      }
    }

    // Now update all editor elements with the new data URLs
    for (const element of this.editorElements) {
      if (["image", "video", "audio"].includes(element.type)) {
        if ("properties" in element && "src" in element.properties) {
          const dataUrl = mediaDataMap.get(element.id);
          if (dataUrl) {
            element.properties.src = dataUrl;

            // Track this as a valid resource
            if (element.type === "image") validImages.add(dataUrl);
            if (element.type === "video") validVideos.add(dataUrl);
            if (element.type === "audio") validAudios.add(dataUrl);
          } else if (typeof element.properties.src === "string") {
            // If we didn't find it in our mediaFiles but it has a src, track it as valid
            // This handles direct src values that might be in the elements
            if (element.type === "image")
              validImages.add(element.properties.src);
            if (element.type === "video")
              validVideos.add(element.properties.src);
            if (element.type === "audio")
              validAudios.add(element.properties.src);
          }
        }
      }
    }

    // For older project formats, restore resources from direct arrays
    // but filter out any that are blob URLs (which are no longer valid)
    const isValidUrl = (url: string) => {
      return (
        url.startsWith("data:") || // Data URLs are preserved
        !url.startsWith("blob:")
      ); // Blob URLs are not valid after reload
    };

    if (Array.isArray(stateObject.audios)) {
      stateObject.audios.forEach((audio: string) => {
        if (isValidUrl(audio) && !this.audios.includes(audio)) {
          this.audios.push(audio);
          validAudios.add(audio);
        }
      });
    }

    if (Array.isArray(stateObject.videos)) {
      stateObject.videos.forEach((video: string) => {
        if (isValidUrl(video) && !this.videos.includes(video)) {
          this.videos.push(video);
          validVideos.add(video);
        }
      });
    }

    if (Array.isArray(stateObject.images)) {
      stateObject.images.forEach((image: string) => {
        if (isValidUrl(image) && !this.images.includes(image)) {
          this.images.push(image);
          validImages.add(image);
        }
      });
    }

    // Clean up any resources that are no longer valid or in use

    // For images, keep only valid ones
    this.images = this.images.filter((image) => validImages.has(image));

    // For videos, keep only valid ones
    this.videos = this.videos.filter((video) => validVideos.has(video));

    // For audios, keep only valid ones
    this.audios = this.audios.filter((audio) => validAudios.has(audio));

    // Remove duplicate entries
    this.images = [...new Set(this.images)];
    this.videos = [...new Set(this.videos)];
    this.audios = [...new Set(this.audios)];

    // Reinitialize the animationTimeLine
    this.animationTimeLine = anime.timeline({ autoplay: false });

    // Refresh elements and animations
    this.refreshElements();
    this.refreshAnimations();
  }

  replaceImageResource(index: number, newImageUrl: string): void {
    // Keep the old URL to properly clean up later
    const oldImageUrl = this.images[index];

    // Replace the image in the images array
    this.images[index] = newImageUrl;

    // Now we need to update any editor elements that use this image
    for (const element of this.editorElements) {
      if (
        element.type === "image" &&
        "properties" in element &&
        "src" in element.properties &&
        element.properties.src === oldImageUrl
      ) {
        // Update the element to use the new image URL
        element.properties.src = newImageUrl;
      }
    }

    // If the oldImageUrl was a blob URL and is no longer used elsewhere,
    // we can revoke it to free up memory
    const isStillInUse =
      this.images.includes(oldImageUrl) ||
      this.editorElements.some(
        (el) =>
          el.type === "image" &&
          "properties" in el &&
          "src" in el.properties &&
          el.properties.src === oldImageUrl
      );

    if (!isStillInUse && oldImageUrl.startsWith("blob:")) {
      URL.revokeObjectURL(oldImageUrl);
    }
  }

  handleKeyboardShortcut(event: KeyboardEvent) {
    if (!this.canvas) return;

    // Check if the target is an input field, textarea, or any element that needs arrow key navigation
    const target = event.target as HTMLElement;
    const isInputElement =
      target.tagName === "INPUT" ||
      target.tagName === "TEXTAREA" ||
      target.isContentEditable ||
      target.tagName === "SELECT";

    // If we're in an input element let the default behavior happen
    if (isInputElement) {
      return;
    }

    const activeObject = this.canvas.getActiveObject();
    const activeGroup = this.canvas.getActiveObjects();

    switch (event.key) {
      case "Delete":
        if (
          (activeObject || activeGroup.length) &&
          (event.ctrlKey || event.metaKey)
        ) {
          if (this.selectedElement) {
            this.deleteSelectedObjects([this.selectedElement]);
          }
          event.preventDefault();
        }
        break;

      case "ArrowUp":
      case "ArrowDown":
      case "ArrowLeft":
      case "ArrowRight":
        // Only handle arrow keys if:
        // 1. Ctrl/Meta is pressed (for moving objects), OR
        // 2. We're not in an input field (for time skipping)
        if (event.ctrlKey || event.metaKey) {
          this.moveSelectedObject(event.key);
          event.preventDefault();
        } else if (!isInputElement) {
          this.skipInTime(event.key);
          event.preventDefault();
        }
        break;

      case "c":
        if ((event.ctrlKey || event.metaKey) && event.altKey) {
          this.copyObject();
          event.preventDefault();
        }
        break;

      case "v":
        if ((event.ctrlKey || event.metaKey) && event.altKey) {
          this.pasteObject();
          event.preventDefault();
        }
        break;

      case " ":{
        this.setPlaying(!this.playing);
        break;
      }

      default:
        break;
    }
  }

  skipInTime(key: string) {
    if (key === "ArrowLeft") {
      this.skipBackward();
    }
    if (key === "ArrowRight") {
      this.skipForward();
    }
  }

  deleteSelectedObjects(selectedObjects: EditorElement[] | null) {
    selectedObjects?.forEach((selectedObject) => {
      if (selectedObject) {
        this.removeEditorElement(selectedObject.id);
      }
    });
  }

  copyObject() {
    const activeObject = this.canvas?.getActiveObject();
    if (activeObject) {
      this.clipboard = this.selectedElement;
    }
  }

  pasteObject() {
    if (!this.clipboard) return;
    // console.log("clipboard:", JSON.stringify(this.clipboard, null, 2));
    const type = this.clipboard.type;
    switch (type) {
      case "text":
        const textInElement = this.clipboard.properties.text;
        const fontSize = this.clipboard.properties.fontSize;
        const fontWeight = this.clipboard.properties.fontWeight;
        this.addText({ text: textInElement, fontSize, fontWeight });
        break;
      case "image":
        const imageIndex = parseInt(
          this.clipboard.name.match(/(\d+)\s*$/)?.toString() || "",
          10
        );
        this.addImage(imageIndex - 1);
        break;
      case "mafs":
        const mafsIndex = parseInt(
          this.clipboard.name.match(/(\d+)\s*$/)?.toString() || "",
          10
        );
        const mafsName = this.clipboard.name.match(/\(([^)]+)\)/)?.[1] || "";
        const mafsPngSrc = this.clipboard.properties.src;
        this.addMafsResource(mafsIndex - 1, mafsPngSrc, mafsName);
        break;
      case "video":
        const videoIndex = parseInt(
          this.clipboard.name.match(/(\d+)\s*$/)?.toString() || "",
          10
        );
        this.addVideo(videoIndex - 1);
        break;
      default:
        break;
    }
  }

  moveSelectedObject(direction: string) {
    const activeObjects = this.canvas?.getActiveObjects() || [];
    const delta = 10; // Movement increment
    activeObjects.forEach((object) => {
      if (direction === "ArrowUp") object.top = (object.top || 0) - delta;
      if (direction === "ArrowDown") object.top = (object.top || 0) + delta;
      if (direction === "ArrowLeft") object.left = (object.left || 0) - delta;
      if (direction === "ArrowRight") object.left = (object.left || 0) + delta;
      object.setCoords();
    });
    this.canvas?.renderAll();
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

  get currentTimeInMs() {
    return (this.currentKeyFrame * 1000) / this.fps;
  }

  setCurrentTimeInMs(time: number) {
    this.currentKeyFrame = Math.floor((time / 1000) * this.fps);
  }

  setSelectedMenuOption(selectedMenuOption: MenuOption) {
    this.selectedMenuOption = selectedMenuOption;
  }

  setCanvas(canvas: fabric.Canvas | null, width: number, height: number) {
    this.canvas = canvas;
    if (canvas) {
      canvas.setWidth(width);
      canvas.setHeight(height);
      canvas.backgroundColor = this.backgroundColor;

      // Add the transparent box to the canvas
      this.createCanvasBox(width, height);

      const guideline = new AlignGuidelines({
        canvas: this.canvas as fabric.Canvas,
        aligningOptions: {
          lineWidth: 5,
          lineColor: "#ff0000",
        },
      });

      guideline.init();
    }

    this.canvas_width = width;
    this.canvas_height = height;
  }

  setCanvasSize(width: number, height: number) {
    if (this.canvas) {
      this.canvas.setWidth(width);
      this.canvas.setHeight(height);
    }
    this.canvas_width = width;
    this.canvas_height = height;

    // Update the size of the canvas box
    this.updateCanvasBoxSize(width, height);
    this.refreshElements();
  }

  createCanvasBox(width: number, height: number) {
    if (!this.canvas) return;

    // Remove existing box if any
    if (this.canvasBox) {
      this.canvas.remove(this.canvasBox);
    }

    // Create a new transparent box
    this.canvasBox = new fabric.Rect({
      left: 0,
      top: 0,
      width: width,
      height: height,
      fill: "transparent", // Fully transparent
      selectable: false, // Prevent selection
      evented: false, // Prevent events like click
    });

    // Add it to the canvas and send to the back
    this.canvas.add(this.canvasBox);
    this.canvasBox.sendToBack();
  }

  updateCanvasBoxSize(width: number, height: number) {
    if (this.canvasBox && this.canvas) {
      this.canvasBox.set({
        width: width,
        height: height,
      });
      this.canvasBox.setCoords(); // Update the object's bounding box
      this.canvas.renderAll(); // Re-render the canvas
    }
  }

  setBackgroundColor(backgroundColor: string) {
    this.backgroundColor = backgroundColor;
    if (this.canvas) {
      this.canvas.backgroundColor = backgroundColor;
    }
  }

  updateEffect(id: string, effect: Effect) {
    const index = this.editorElements.findIndex((element) => element.id === id);
    const element = this.editorElements[index];
    if (
      isEditorVideoElement(element) ||
      isEditorImageElement(element) ||
      isEditorMafsElement(element)
    ) {
      element.properties.effect = effect;
    }
    this.refreshElements();
  }

  setVideos(videos: string[]) {
    this.videos = videos;
  }

  addVideoResource(video: string) {
    this.videos = [...this.videos, video];
  }
  addAudioResource(audio: string) {
    this.audios = [...this.audios, audio];
  }
  addImageResource(image: string) {
    this.images = [...this.images, image];
  }

  addAnimation(animation: Animation) {
    this.animations = [...this.animations, animation];
    this.refreshAnimations();
  }
  updateAnimation(id: string, animation: Animation) {
    const index = this.animations.findIndex((a) => a.id === id);
    this.animations[index] = animation;
    this.refreshAnimations();
  }

  refreshAnimations() {
    anime.remove(this.animationTimeLine);
    this.animationTimeLine = anime.timeline({
      duration: this.maxTime,
      autoplay: false,
    });
    for (let i = 0; i < this.animations.length; i++) {
      const animation = this.animations[i];
      const editorElement = this.editorElements.find(
        (element) => element.id === animation.targetId
      );
      const fabricObject = editorElement?.fabricObject;
      if (!editorElement || !fabricObject) {
        continue;
      }
      fabricObject.clipPath = undefined;
      switch (animation.type) {
        case "fadeIn": {
          this.animationTimeLine.add(
            {
              opacity: [0, 1],
              duration: animation.duration,
              targets: fabricObject,
              easing: "linear",
            },
            editorElement.timeFrame.start
          );
          break;
        }
        case "fadeOut": {
          this.animationTimeLine.add(
            {
              opacity: [1, 0],
              duration: animation.duration,
              targets: fabricObject,
              easing: "linear",
            },
            editorElement.timeFrame.end - animation.duration
          );
          break;
        }
        case "slideIn": {
          const direction = animation.properties.direction;
          const targetPosition = {
            left: editorElement.placement.x,
            top: editorElement.placement.y,
          };
          const startPosition = {
            left:
              direction === "left"
                ? -editorElement.placement.width
                : direction === "right"
                ? this.canvas?.width
                : editorElement.placement.x,
            top:
              direction === "top"
                ? -editorElement.placement.height
                : direction === "bottom"
                ? this.canvas?.height
                : editorElement.placement.y,
          };
          if (animation.properties.useClipPath) {
            const clipRectangle = FabricUtils.getClipMaskRect(
              editorElement,
              50
            );
            fabricObject.set("clipPath", clipRectangle);
          }
          if (
            editorElement.type === "text" &&
            animation.properties.textType === "character"
          ) {
            this.canvas?.remove(...editorElement.properties.splittedTexts);
            // @ts-ignore\
            editorElement.properties.splittedTexts =
              editorElement.fabricObject instanceof fabric.Text
                ? getTextObjectsPartitionedByCharacters(
                    editorElement.fabricObject as fabric.Text,
                    editorElement
                  )
                : editorElement.properties.splittedTexts;

            editorElement.properties.splittedTexts.forEach((textObject) => {
              this.canvas!.add(textObject);
            });
            const duration = animation.duration / 2;
            const delay =
              duration / editorElement.properties.splittedTexts.length;
            for (
              let i = 0;
              i < editorElement.properties.splittedTexts.length;
              i++
            ) {
              const splittedText = editorElement.properties.splittedTexts[i];
              const offset = {
                left: splittedText.left! - editorElement.placement.x,
                top: splittedText.top! - editorElement.placement.y,
              };
              this.animationTimeLine.add(
                {
                  left: [
                    startPosition.left! + offset.left,
                    targetPosition.left + offset.left,
                  ],
                  top: [
                    startPosition.top! + offset.top,
                    targetPosition.top + offset.top,
                  ],
                  delay: i * delay,
                  duration: duration,
                  targets: splittedText,
                },
                editorElement.timeFrame.start
              );
            }
            this.animationTimeLine.add(
              {
                opacity: [1, 0],
                duration: 1,
                targets: fabricObject,
                easing: "linear",
              },
              editorElement.timeFrame.start
            );
            this.animationTimeLine.add(
              {
                opacity: [0, 1],
                duration: 1,
                targets: fabricObject,
                easing: "linear",
              },
              editorElement.timeFrame.start + animation.duration
            );

            this.animationTimeLine.add(
              {
                opacity: [0, 1],
                duration: 1,
                targets: editorElement.properties.splittedTexts,
                easing: "linear",
              },
              editorElement.timeFrame.start
            );
            this.animationTimeLine.add(
              {
                opacity: [1, 0],
                duration: 1,
                targets: editorElement.properties.splittedTexts,
                easing: "linear",
              },
              editorElement.timeFrame.start + animation.duration
            );
          }
          this.animationTimeLine.add(
            {
              left: [startPosition.left, targetPosition.left],
              top: [startPosition.top, targetPosition.top],
              duration: animation.duration,
              targets: fabricObject,
              easing: "linear",
            },
            editorElement.timeFrame.start
          );
          break;
        }
        case "slideOut": {
          const direction = animation.properties.direction;
          const startPosition = {
            left: editorElement.placement.x,
            top: editorElement.placement.y,
          };
          const targetPosition = {
            left:
              direction === "left"
                ? -editorElement.placement.width
                : direction === "right"
                ? this.canvas?.width
                : editorElement.placement.x,
            top:
              direction === "top"
                ? -100 - editorElement.placement.height
                : direction === "bottom"
                ? this.canvas?.height
                : editorElement.placement.y,
          };
          if (animation.properties.useClipPath) {
            const clipRectangle = FabricUtils.getClipMaskRect(
              editorElement,
              50
            );
            fabricObject.set("clipPath", clipRectangle);
          }
          this.animationTimeLine.add(
            {
              left: [startPosition.left, targetPosition.left],
              top: [startPosition.top, targetPosition.top],
              duration: animation.duration,
              targets: fabricObject,
              easing: "linear",
            },
            editorElement.timeFrame.end - animation.duration
          );
          break;
        }
        case "breathe": {
          const itsSlideInAnimation = this.animations.find(
            (a) => a.targetId === animation.targetId && a.type === "slideIn"
          );
          const itsSlideOutAnimation = this.animations.find(
            (a) => a.targetId === animation.targetId && a.type === "slideOut"
          );
          const timeEndOfSlideIn = itsSlideInAnimation
            ? editorElement.timeFrame.start + itsSlideInAnimation.duration
            : editorElement.timeFrame.start;
          const timeStartOfSlideOut = itsSlideOutAnimation
            ? editorElement.timeFrame.end - itsSlideOutAnimation.duration
            : editorElement.timeFrame.end;
          if (timeEndOfSlideIn > timeStartOfSlideOut) {
            continue;
          }
          const duration = timeStartOfSlideOut - timeEndOfSlideIn;
          const easeFactor = 4;
          const suitableTimeForHeartbeat = ((1000 * 60) / 72) * easeFactor;
          const upScale = 1.05;
          const currentScaleX = fabricObject.scaleX ?? 1;
          const currentScaleY = fabricObject.scaleY ?? 1;
          const finalScaleX = currentScaleX * upScale;
          const finalScaleY = currentScaleY * upScale;
          const totalHeartbeats = Math.floor(
            duration / suitableTimeForHeartbeat
          );
          if (totalHeartbeats < 1) {
            continue;
          }
          const keyframes = [];
          for (let i = 0; i < totalHeartbeats; i++) {
            keyframes.push({ scaleX: finalScaleX, scaleY: finalScaleY });
            keyframes.push({ scaleX: currentScaleX, scaleY: currentScaleY });
          }

          this.animationTimeLine.add(
            {
              duration: duration,
              targets: fabricObject,
              keyframes,
              easing: "linear",
              loop: true,
            },
            timeEndOfSlideIn
          );

          break;
        }
      }
    }
  }

  removeAnimation(id: string) {
    this.animations = this.animations.filter(
      (animation) => animation.id !== id
    );
    this.refreshAnimations();
  }

  setSelectedElement(selectedElement: EditorElement | null) {
    this.selectedElement = selectedElement;
    if (this.canvas) {
      if (selectedElement?.fabricObject)
        this.canvas.setActiveObject(selectedElement.fabricObject);
      else this.canvas.discardActiveObject();
    }
  }
  updateSelectedElement() {
    this.selectedElement =
      this.editorElements.find(
        (element) => element.id === this.selectedElement?.id
      ) ?? null;
  }

  setEditorElements(editorElements: EditorElement[]) {
    // console.log("Updating editor elements:", editorElements);
    this.editorElements = editorElements;
    this.updateSelectedElement();
    // console.log("Updated selected element:", this.selectedElement);
    // console.log("Calling refreshElements");
    this.refreshElements();
    // console.log("Calling refreshAnimations");
    this.refreshAnimations();
  }

  async updateEditorElement(editorElement: EditorElement): Promise<void> {
    try {
      // console.log("Updating editor element:", editorElement);

      await this.setEditorElements(
        this.editorElements.map((element) =>
          element.id === editorElement.id ? editorElement : element
        )
      );

      this.updateSelectedElement();
      this.refreshElements();
    } catch (error) {
      console.error("Failed to update editor element:", error);
      throw error; // Rethrow the error to propagate it to the caller
    }
  }

  async updateEditorElementTimeFrame(
    editorElement: EditorElement,
    timeFrame: Partial<TimeFrame>
  ): Promise<void> {
    try {
      if (timeFrame.start === undefined && timeFrame.end === undefined) {
        return;
      }

      if (timeFrame.start !== undefined && timeFrame.start < 0) {
        timeFrame.start = 0;
      }
      if (timeFrame.end !== undefined && timeFrame.end > this.maxTime) {
        timeFrame.end = this.maxTime;
      }

      // console.log("Updating time frame for element:", editorElement);
      // console.log("New time frame values:", timeFrame);

      const newEditorElement = {
        ...editorElement,
        timeFrame: {
          ...editorElement.timeFrame,
          ...timeFrame,
        },
      };

      this.updateVideoElements();
      this.updateAudioElements();

      await this.updateEditorElement(newEditorElement);
      this.refreshAnimations();
    } catch (error) {
      console.error("Failed to update editor element time frame:", error);
      throw error; // Rethrow the error to propagate it to the caller
    }
  }

  addEditorElement(editorElement: EditorElement) {
    return new Promise<void>((resolve) => {
      this.setEditorElements([...this.editorElements, editorElement]);
      // console.log("Added editor element:", editorElement);
      this.refreshElements();
      this.setSelectedElement(
        this.editorElements[this.editorElements.length - 1]
      );
      resolve();
    });
  }

  removeEditorElement(id: string) {
    this.setEditorElements(
      this.editorElements.filter((editorElement) => editorElement.id !== id)
    );
    this.refreshElements();
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
      // console.log("Started time:", this.startedTime);
      // console.log("Started time play:", this.startedTimePlay);

      requestAnimationFrame(() => {
        this.playFrames();
      });
    }
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

  startedTime = 0;
  startedTimePlay = 0;

  playFrames() {
    if (!this.playing) {
      return;
    }
    const elapsedTime = Date.now() - this.startedTime;
    const newTime = this.startedTimePlay + elapsedTime;
    this.updateTimeTo(newTime);
    if (newTime > this.maxTime) {
      this.currentKeyFrame = 0;
      this.setPlaying(false);
    } else {
      // console.log("New time:", newTime);
      // console.log("Current key frame:", this.currentKeyFrame);
      requestAnimationFrame(() => {
        this.playFrames();
      });
    }
  }
  updateTimeTo(newTime: number) {
    this.setCurrentTimeInMs(newTime);
    this.animationTimeLine?.seek(newTime);
    if (this.canvas) {
      this.canvas.backgroundColor = this.backgroundColor;
    }
    this.editorElements.forEach((e) => {
      if (!e.fabricObject) return;
      const isInside =
        e.timeFrame.start <= newTime && newTime <= e.timeFrame.end;
      e.fabricObject.visible = isInside;
    });
  }

  handleSeek(seek: number) {
    if (this.playing) {
      this.setPlaying(false);
    }
    this.updateTimeTo(seek);
    this.updateVideoElements();
    this.updateAudioElements();
  }

  addVideo(index: number) {
    const videoElement = document.getElementById(`video-${index}`);
    if (!isHtmlVideoElement(videoElement)) {
      return;
    }
    const videoDurationMs = videoElement.duration * 1000;
    const aspectRatio = videoElement.videoWidth / videoElement.videoHeight;
    const videoId = getUid();
    const audioId = getUid();

    // Create video element without sound
    this.addEditorElement({
      id: videoId,
      name: `Media(video) ${index + 1}`,
      type: "video",
      placement: {
        x: 0,
        y: 0,
        width: 100 * aspectRatio,
        height: 100,
        rotation: 0,
        scaleX: 1,
        scaleY: 1,
      },
      timeFrame: {
        start: 0,
        end: videoDurationMs,
      },
      properties: {
        elementId: `video-${videoId}`,
        src: videoElement.src,
        effect: {
          type: "none",
        },
        muted: true, // Mute the video element
      },
    });

    // Create separate audio element
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
      timeFrame: {
        start: 0,
        end: videoDurationMs,
      },
      properties: {
        elementId: `audio-${audioId}`,
        src: videoElement.src,
      },
    });
  }

  addImage(index: number) {
    const imageElement = document.getElementById(`image-${index}`);
    if (!isHtmlImageElement(imageElement)) {
      return;
    }
    const aspectRatio = imageElement.naturalWidth / imageElement.naturalHeight;
    const id = getUid();
    this.addEditorElement({
      id,
      name: `Media(image) ${index + 1}`,
      type: "image",
      placement: {
        x: 0,
        y: 0,
        width: 100 * aspectRatio,
        height: 100,
        rotation: 0,
        scaleX: 1,
        scaleY: 1,
      },
      timeFrame: {
        start: 0,
        end: this.maxTime,
      },
      properties: {
        elementId: `image-${id}`,
        src: imageElement.src,
        effect: {
          type: "none",
        },
      },
    });
  }

  addMafsResource(index: number, pngSrc: string, name: string) {
    // console.log("index", index);
    // console.log("pngSrc", pngSrc);
    // console.log("name", name);
    return new Promise<void>((resolve, reject) => {
      const img = new Image();
      img.crossOrigin = "anonymous";
      img.onload = () => {
        const id = getUid();
        // console.log("Generated ID:", id);
        // console.log("Dimensions:", img.width, img.height);

        const coverImage = new fabric.CoverImage(img, {
          name: `Mafs(${name}) ${index + 1}`,
          left: 0,
          top: 0,
          scaleX: 1,
          scaleY: 1,
          angle: 0,
          width: img.width,
          height: img.height,
          originX: "left",
          originY: "top",
          selectable: false,
          crossOrigin: "anonymous",
          objectCaching: false,
          lockUniScaling: true,
          customFilter: "none",
        } as any);

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
          timeFrame: {
            start: 0,
            end: this.maxTime,
          },
          properties: {
            elementId: `mafs-${id}`,
            src: pngSrc,
            effect: {
              type: "none",
            },
            imageObject: coverImage,
          },
        });

        // console.log("Element added:", coverImage);

        resolve();
      };
      img.onerror = reject;
      img.src = pngSrc;
    });
  }

  addAudio(index: number) {
    const audioElement = document.getElementById(`audio-${index}`);
    if (!isHtmlAudioElement(audioElement)) {
      return;
    }
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
      timeFrame: {
        start: 0,
        end: audioDurationMs,
      },
      properties: {
        elementId: `audio-${id}`,
        src: audioElement.src,
      },
    });
  }
  addText(options: { text: string; fontSize: number; fontWeight: number }) {
    const id = getUid();
    const index = this.editorElements.length;
    this.addEditorElement({
      id,
      name: `Text ${index + 1}`,
      type: "text",
      placement: {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
        rotation: 0,
        scaleX: 1,
        scaleY: 1,
      },
      timeFrame: {
        start: 0,
        end: this.maxTime,
      },
      properties: {
        text: options.text,
        fontSize: options.fontSize,
        fontWeight: options.fontWeight,
        splittedTexts: [],
      },
    });
  }

  updateVideoElements() {
    this.editorElements
      .filter(
        (element): element is VideoEditorElement => element.type === "video"
      )
      .forEach((element) => {
        const video = document.getElementById(element.properties.elementId);
        if (isHtmlVideoElement(video)) {
          const videoTime =
            (this.currentTimeInMs - element.timeFrame.start) / 1000;
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
    this.editorElements
      .filter(
        (element): element is AudioEditorElement => element.type === "audio"
      )
      .forEach((element) => {
        const audio = document.getElementById(element.properties.elementId);
        if (isHtmlAudioElement(audio)) {
          const audioTime =
            (this.currentTimeInMs - element.timeFrame.start) / 1000;
          audio.currentTime = audioTime;
          if (this.playing) {
            audio.play();
          } else {
            audio.pause();
          }
        }
      });
  }
  // saveCanvasToVideo() {
  //   const video = document.createElement("video");
  //   const canvas = document.getElementById("canvas") as HTMLCanvasElement;
  //   const stream = canvas.captureStream();
  //   video.srcObject = stream;
  //   video.play();
  //   const mediaRecorder = new MediaRecorder(stream);
  //   const chunks: Blob[] = [];
  //   mediaRecorder.ondataavailable = function (e) {
  //     console.log("data available");
  //     console.log(e.data);
  //     chunks.push(e.data);
  //   };
  //   mediaRecorder.onstop = function (e) {
  //     const blob = new Blob(chunks, { type: "video/webm" });
  //     const url = URL.createObjectURL(blob);
  //     const a = document.createElement("a");
  //     a.href = url;
  //     a.download = "video.webm";
  //     a.click();
  //   };
  //   mediaRecorder.start();
  //   setTimeout(() => {
  //     mediaRecorder.stop();
  //   }, this.maxTime);

  // }

  setVideoFormat(format: "mp4" | "webm") {
    this.selectedVideoFormat = format;
  }

  saveCanvasToVideoWithAudio() {
    this.saveCanvasToVideoWithAudioWebmMp4();
  }

  saveCanvasToVideoWithAudioWebmMp4() {
    // console.log("modified");
    let mp4 = this.selectedVideoFormat === "mp4";

    const canvas = document.getElementById("canvas") as HTMLCanvasElement;
    const stream = canvas.captureStream(30);
    const audioElements = this.editorElements.filter(isEditorAudioElement);
    const audioStreams: MediaStream[] = [];

    // Remove previously added audio tracks from the stream
    stream.getAudioTracks().forEach((track) => {
      stream.removeTrack(track);
    });

    audioElements.forEach((audio) => {
      const audioElement = document.getElementById(
        audio.properties.elementId
      ) as HTMLAudioElement;
      if (audioElement) {
        const { context, sourceNode } = this.getAudioContext(audioElement);
        let dest = context.createMediaStreamDestination();
        sourceNode.connect(dest);
        audioStreams.push(dest.stream);
      }
    });

    // Check if there are any audio streams available
    if (audioStreams.length > 0) {
      // Create a new AudioContext and AudioDestinationNode for mixing the audio streams
      const mixerContext = new AudioContext();
      const mixerDestination = mixerContext.createMediaStreamDestination();

      audioStreams.forEach((audioStream) => {
        const sourceNode = mixerContext.createMediaStreamSource(audioStream);
        sourceNode.connect(mixerDestination);
      });

      // Add the mixed audio stream to the main stream
      stream.addTrack(mixerDestination.stream.getAudioTracks()[0]);
    }

    const video = document.createElement("video");
    video.srcObject = stream;
    video.height = 500;
    video.width = 800;
    // video.controls = true;
    // document.body.appendChild(video);
    video.play().then(() => {
      const mediaRecorder = new MediaRecorder(stream);
      const chunks: Blob[] = [];
      mediaRecorder.ondataavailable = function (e) {
        chunks.push(e.data);
        // console.log("data available");
      };
      mediaRecorder.onstop = async function (e) {
        // const blob = new Blob(chunks, { type: "video/webm" });
        const blob = await fixWebmDuration(
          new Blob([...chunks], { type: "video/webm" })
        );

        if (mp4) {
          // lets use ffmpeg to convert webm to mp4
          const data = new Uint8Array(await blob.arrayBuffer());
          const ffmpeg = new FFmpeg();
          const baseURL = "https://unpkg.com/@ffmpeg/core@0.12.2/dist/umd";
          await ffmpeg.load({
            coreURL: await toBlobURL(
              `${baseURL}/ffmpeg-core.js`,
              "text/javascript"
            ),
            wasmURL: await toBlobURL(
              `${baseURL}/ffmpeg-core.wasm`,
              "application/wasm"
            ),
            // workerURL: await toBlobURL(`${baseURL}/ffmpeg-core.worker.js`, 'text/javascript'),
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
          // await ffmpeg.exec(["-y", "-i", "video.webm", "-c:v", "libx264", "video.mp4"]);

          const output = await ffmpeg.readFile("video.mp4");
          const outputBlob = new Blob([output], { type: "video/mp4" });
          const outputUrl = URL.createObjectURL(outputBlob);
          const a = document.createElement("a");
          a.download = "video.mp4";
          a.href = outputUrl;
          a.click();
        } else {
          // TODO: Add length as metadata to the webm video

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
      }, this.maxTime);
      video.remove();
    });
  }

  refreshElements() {
    const state = this;
    if (!state.canvas) return;
    const canvas = state.canvas;

    // Update canvas dimensions
    canvas.setWidth(state.canvas_width);
    canvas.setHeight(state.canvas_height);

    // Deselect all objects before removing them
    canvas.discardActiveObject();
    canvas
      .getObjects()
      .filter((obj) => obj !== state.canvasBox)
      .forEach((obj) => {
        obj.onDeselect = () => false; // Release deselect
      });
    const objectsToRemove = canvas
      .getObjects()
      .filter((obj) => obj !== state.canvasBox);
    state.canvas.remove(...objectsToRemove);

    const handleObjectModified =
      (element: any, fabricObject: any) => (e: any) => {
        if (!e.target) return;
        const target = e.target;
        if (target !== fabricObject) return;
        const placement = element.placement;
        const newPlacement: Placement = {
          ...placement,
          x: target.left ?? placement.x,
          y: target.top ?? placement.y,
          rotation: target.angle ?? placement.rotation,
          width: target.width ?? placement.width,
          height: target.height ?? placement.height,
          scaleX: target.scaleX ?? placement.scaleX,
          scaleY: target.scaleY ?? placement.scaleY,
        };

        const newElement = {
          ...element,
          placement: newPlacement,
          properties: {
            ...element.properties,
            src: element.properties.src,
            text: fabricObject.text ?? element.properties.text,
          },
        };
        state.updateEditorElement(newElement);
      };

    const addElementToCanvas = (
      element: any,
      fabricObject: any,
      setActive = true
    ) => {
      element.fabricObject = fabricObject;
      if (element && element.fabricObject) {
        canvas.add(element.fabricObject);
      }
      fabricObject.on("modified", handleObjectModified(element, fabricObject));
      fabricObject.on("selected", () => {
        state.setSelectedElement(element);
      });
      if (setActive) {
        canvas.setActiveObject(fabricObject);
      }
      canvas.renderAll();
    };

    const loadMafsImage = (element: any) => {
      return new Promise<void>((resolve, reject) => {
        const img = new Image();
        img.crossOrigin = "anonymous";
        img.onload = () => {
          try {
            const imageObject = new fabric.CoverImage(img, {
              name: element.id,
              left: element.placement.x,
              top: element.placement.y,
              angle: element.placement.rotation,
              objectCaching: false,
              selectable: true,
              lockUniScaling: true,
              customFilter: element.properties.effect?.type as any,
            } as any);

            const image = {
              w: img.naturalWidth,
              h: img.naturalHeight,
            };

            imageObject.width = image.w;
            imageObject.height = image.h;

            img.width = image.w;
            img.height = image.h;

            imageObject.scaleToHeight(image.h);
            imageObject.scaleToWidth(image.w);

            const toScale = {
              x: element.placement.width / image.w,
              y: element.placement.height / image.h,
            };

            imageObject.scaleX = toScale.x * element.placement.scaleX;
            imageObject.scaleY = toScale.y * element.placement.scaleY;

            element.fabricObject = imageObject;
            element.properties.imageObject = imageObject;

            addElementToCanvas(element, imageObject, false);
            resolve();
          } catch (error) {
            console.error("Error loading Mafs image:", error);
            reject(error);
          }
        };
        img.onerror = reject;
        img.src = element.properties.src;
      });
    };

    const elementPromises = state.editorElements.map(async (element: any) => {
      switch (element.type) {
        case "video": {
          const videoElement = document.getElementById(
            element.properties.elementId
          );
          if (!videoElement || !isHtmlVideoElement(videoElement)) return;
          const videoObject = new fabric.CoverVideo(videoElement, {
            name: element.id,
            left: element.placement.x,
            top: element.placement.y,
            width: element.placement.width,
            height: element.placement.height,
            scaleX: element.placement.scaleX,
            scaleY: element.placement.scaleY,
            angle: element.placement.rotation,
            objectCaching: false,
            selectable: true,
            lockUniScaling: true,
            customFilter: element.properties.effect?.type as any,
          } as any);
          videoElement.width = 100;
          videoElement.height =
            (videoElement.videoHeight * 100) / videoElement.videoWidth;
          videoElement.muted = true; // Mute the video element
          addElementToCanvas(element, videoObject, false);
          break;
        }
        case "mafs": {
          await loadMafsImage(element);
          break;
        }
        case "image": {
          const imageElement = document.getElementById(
            element.properties.elementId
          );
          if (!imageElement || !isHtmlImageElement(imageElement)) return;
          const imageObject = new fabric.CoverImage(imageElement, {
            name: element.id,
            left: element.placement.x,
            top: element.placement.y,
            angle: element.placement.rotation,
            objectCaching: false,
            selectable: true,
            lockUniScaling: true,
            customFilter: element.properties.effect?.type as any,
          } as any);
          const image = {
            w: imageElement.naturalWidth,
            h: imageElement.naturalHeight,
          };
          imageObject.width = image.w;
          imageObject.height = image.h;
          imageElement.width = image.w;
          imageElement.height = image.h;
          imageObject.scaleToHeight(image.w);
          imageObject.scaleToWidth(image.h);
          const toScale = {
            x: element.placement.width / image.w,
            y: element.placement.height / image.h,
          };
          imageObject.scaleX = toScale.x * element.placement.scaleX;
          imageObject.scaleY = toScale.y * element.placement.scaleY;
          addElementToCanvas(element, imageObject, false);
          break;
        }
        case "audio": {
          break;
        }
        case "text": {
          const textObject = new fabric.Textbox(element.properties.text, {
            name: element.id,
            left: element.placement.x,
            top: element.placement.y,
            scaleX: element.placement.scaleX,
            scaleY: element.placement.scaleY,
            width: element.placement.width,
            height: element.placement.height,
            angle: element.placement.rotation,
            fontSize: element.properties.fontSize,
            fontWeight: element.properties.fontWeight,
            objectCaching: false,
            selectable: true,
            lockUniScaling: true,
            fill: "#ffffff",
          });
          addElementToCanvas(element, textObject, false);
          break;
        }
        default: {
          throw new Error("Not implemented");
        }
      }
    });
    Promise.all(elementPromises)
      .then(() => {
        const selectedEditorElement = state.selectedElement;
        if (selectedEditorElement && selectedEditorElement.fabricObject) {
          canvas.setActiveObject(selectedEditorElement.fabricObject);
        }
        this.refreshAnimations();
        this.updateTimeTo(this.currentTimeInMs);
        canvas.renderAll();
      })
      .catch((error) => {
        console.error("Error processing elements:", error);
      });
  }
}

export function isEditorAudioElement(
  element: EditorElement
): element is AudioEditorElement {
  return element.type === "audio";
}
export function isEditorVideoElement(
  element: EditorElement
): element is VideoEditorElement {
  return element.type === "video";
}

export function isEditorImageElement(
  element: EditorElement
): element is ImageEditorElement {
  return element.type === "image";
}

export function isEditorMafsElement(
  element: EditorElement
): element is MafsEditorElement {
  return element.type === "mafs";
}

function getTextObjectsPartitionedByCharacters(
  textObject: fabric.Text,
  element: TextEditorElement
): fabric.Text[] {
  let copyCharsObjects: fabric.Text[] = [];
  // replace all line endings with blank
  const characters = (textObject.text ?? "")
    .split("")
    .filter((m) => m !== "\n");
  const charObjects = textObject.__charBounds;
  if (!charObjects) return [];
  const charObjectFixed = charObjects
    .map((m, index) => m.slice(0, m.length - 1).map((m) => ({ m, index })))
    .flat();
  const lineHeight = textObject.getHeightOfLine(0);
  for (let i = 0; i < characters.length; i++) {
    if (!charObjectFixed[i]) continue;
    const { m: charObject, index: lineIndex } = charObjectFixed[i];
    const char = characters[i];
    const scaleX = textObject.scaleX ?? 1;
    const scaleY = textObject.scaleY ?? 1;
    const charTextObject = new fabric.Text(char, {
      left: charObject.left * scaleX + element.placement.x,
      scaleX: scaleX,
      scaleY: scaleY,
      top: lineIndex * lineHeight * scaleY + element.placement.y,
      fontSize: textObject.fontSize,
      fontWeight: textObject.fontWeight,
      fill: "#fff",
    });
    copyCharsObjects.push(charTextObject);
  }
  return copyCharsObjects;
}
