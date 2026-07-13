import { makeAutoObservable } from "mobx";
import { createTimeline, Timeline } from "animejs";
import Konva from "konva";
import type { RootStore } from "../RootStore";
import { Animation, TextEditorElement } from "@/types";
import { KonvaAnimProxy } from "@/utils/konva-utils";

export class AnimationStore {
  animations: Animation[] = [];
  animationTimeLine: Timeline | null = null;

  constructor(private root: RootStore) {
    makeAutoObservable(this, {
      animationTimeLine: false,
    });
    if (typeof window !== "undefined") {
      this.animationTimeLine = createTimeline({ autoplay: false });
    }
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

  removeAnimation(id: string) {
    this.animations = this.animations.filter((animation) => animation.id !== id);
    this.refreshAnimations();
  }

  /** Replace all animations (e.g. new project / load). */
  setAnimations(animations: Animation[]) {
    this.animations = animations;
    this.refreshAnimations();
  }

  refreshAnimations() {
    const { editorElements, konvaNodes } = this.root.elementStore;
    const layer = this.root.canvasStore.layer;

    this.animationTimeLine?.revert();
    this.animationTimeLine = createTimeline({
      defaults: { duration: this.root.playbackStore.maxTime },
      autoplay: false,
    });

    for (let i = 0; i < this.animations.length; i++) {
      const animation = this.animations[i];
      const editorElement = editorElements.find(
        (element) => element.id === animation.targetId
      );
      const konvaNode = editorElement ? konvaNodes.get(editorElement.id) : undefined;

      if (!editorElement || !konvaNode || !layer) {
        continue;
      }

      const proxy = new KonvaAnimProxy(konvaNode, layer);

      switch (animation.type) {
        case "fadeIn": {
          this.animationTimeLine!.add(
            proxy,
            {
              opacity: [0, 1],
              duration: animation.duration,
              ease: "linear",
            },
            editorElement.timeFrame.start
          );
          break;
        }
        case "fadeOut": {
          this.animationTimeLine!.add(
            proxy,
            {
              opacity: [1, 0],
              duration: animation.duration,
              ease: "linear",
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
                  ? this.root.canvasStore.canvas_width
                  : editorElement.placement.x,
            top:
              direction === "top"
                ? -editorElement.placement.height
                : direction === "bottom"
                  ? this.root.canvasStore.canvas_height
                  : editorElement.placement.y,
          };

          if (
            editorElement.type === "text" &&
            animation.properties.textType === "character"
          ) {
            this._addCharacterSlideAnimation(
              editorElement as TextEditorElement,
              startPosition,
              targetPosition,
              animation.duration,
              layer
            );
          }

          this.animationTimeLine!.add(
            proxy,
            {
              left: [startPosition.left, targetPosition.left],
              top: [startPosition.top, targetPosition.top],
              duration: animation.duration,
              ease: "linear",
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
                  ? this.root.canvasStore.canvas_width
                  : editorElement.placement.x,
            top:
              direction === "top"
                ? -100 - editorElement.placement.height
                : direction === "bottom"
                  ? this.root.canvasStore.canvas_height
                  : editorElement.placement.y,
          };

          this.animationTimeLine!.add(
            proxy,
            {
              left: [startPosition.left, targetPosition.left],
              top: [startPosition.top, targetPosition.top],
              duration: animation.duration,
              ease: "linear",
            },
            editorElement.timeFrame.end - animation.duration
          );
          break;
        }
        case "mafsReveal": {
          const direction = animation.properties.direction;
          if (direction === "in") {
            this.animationTimeLine!.add(
              proxy,
              {
                scaleX: [0, konvaNode.scaleX() ?? 1],
                scaleY: [0, konvaNode.scaleY() ?? 1],
                opacity: [0, 1],
                duration: animation.duration,
                ease: "outElastic(1, 0.5)",
              },
              editorElement.timeFrame.start
            );
          } else {
            this.animationTimeLine!.add(
              proxy,
              {
                scaleX: [konvaNode.scaleX() ?? 1, 0],
                scaleY: [konvaNode.scaleY() ?? 1, 0],
                opacity: [1, 0],
                duration: animation.duration,
                ease: "inBack",
              },
              editorElement.timeFrame.end - animation.duration
            );
          }
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

          if (timeEndOfSlideIn > timeStartOfSlideOut) continue;

          const duration = timeStartOfSlideOut - timeEndOfSlideIn;
          const easeFactor = 4;
          const suitableTimeForHeartbeat = ((1000 * 60) / 72) * easeFactor;
          const upScale = 1.05;
          const currentScaleX = konvaNode.scaleX() ?? 1;
          const currentScaleY = konvaNode.scaleY() ?? 1;
          const finalScaleX = currentScaleX * upScale;
          const finalScaleY = currentScaleY * upScale;
          const totalHeartbeats = Math.floor(duration / suitableTimeForHeartbeat);

          if (totalHeartbeats < 1) continue;

          const keyframes = [];
          for (let k = 0; k < totalHeartbeats; k++) {
            keyframes.push({ scaleX: finalScaleX, scaleY: finalScaleY });
            keyframes.push({ scaleX: currentScaleX, scaleY: currentScaleY });
          }

          this.animationTimeLine!.add(
            proxy,
            {
              duration,
              keyframes,
              ease: "linear",
              loop: true,
            },
            timeEndOfSlideIn
          );
          break;
        }
      }
    }
  }

  private _addCharacterSlideAnimation(
    element: TextEditorElement,
    startPosition: { left: number; top: number },
    targetPosition: { left: number; top: number },
    animationDuration: number,
    layer: Konva.Layer
  ) {
    // Remove previously added character text nodes
    element.properties.splittedTexts.forEach((t) => t.destroy());

    const konvaNode = this.root.elementStore.konvaNodes.get(element.id);
    if (!(konvaNode instanceof Konva.Text)) return;

    const characters = (element.properties.text ?? "").split("").filter((m) => m !== "\n");
    const charWidth = element.placement.width / Math.max(characters.length, 1);
    const charTextNodes: Konva.Text[] = [];

    characters.forEach((char, i) => {
      const charNode = new Konva.Text({
        text: char,
        x: element.placement.x + i * charWidth,
        y: element.placement.y,
        fontSize: element.properties.fontSize,
        fontStyle: String(element.properties.fontWeight),
        fill: "#ffffff",
        opacity: 0,
      });
      layer.add(charNode);
      charTextNodes.push(charNode);

      const offset = { left: i * charWidth, top: 0 };
      const duration = animationDuration / 2;
      const delay = duration / characters.length;

      this.animationTimeLine!.add(
        new KonvaAnimProxy(charNode, layer),
        {
          left: [startPosition.left + offset.left, targetPosition.left + offset.left],
          top: [startPosition.top + offset.top, targetPosition.top + offset.top],
          opacity: [0, 1],
          delay: i * delay,
          duration,
        },
        element.timeFrame.start
      );
    });

    // Hide the original text node during character animation
    const proxy = new KonvaAnimProxy(konvaNode, layer);
    this.animationTimeLine!.add(
      proxy,
      { opacity: [1, 0], duration: 1, ease: "linear" },
      element.timeFrame.start
    );
    this.animationTimeLine!.add(
      proxy,
      { opacity: [0, 1], duration: 1, ease: "linear" },
      element.timeFrame.start + animationDuration
    );

    element.properties.splittedTexts = charTextNodes;
  }
}
