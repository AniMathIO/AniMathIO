import { makeAutoObservable } from "mobx";
import Konva from "konva";
import type { RootStore } from "../RootStore";
import { EditorElement } from "@/types";

export class CanvasStore {
  stage: Konva.Stage | null = null;
  layer: Konva.Layer | null = null;
  canvas_width: number = 800;
  canvas_height: number = 600;
  backgroundColor: string = "#111111";

  constructor(private root: RootStore) {
    makeAutoObservable(this, {
      stage: false,
      layer: false,
    });
  }

  setStage(stage: Konva.Stage | null, layer: Konva.Layer | null, width: number, height: number) {
    this.stage = stage;
    this.layer = layer;
    if (stage) {
      stage.width(width);
      stage.height(height);
    }
    this.canvas_width = width;
    this.canvas_height = height;
  }

  setCanvasSize(width: number, height: number) {
    const oldWidth = this.canvas_width;
    const oldHeight = this.canvas_height;

    if (this.stage) {
      this.stage.width(width);
      this.stage.height(height);
    }

    // Proportionally reposition and resize all elements when canvas dimensions change
    if (oldWidth > 0 && oldHeight > 0 && (oldWidth !== width || oldHeight !== height)) {
      const scaleX = width / oldWidth;
      const scaleY = height / oldHeight;
      this.root.elementStore.editorElements = this.root.elementStore.editorElements.map(
        (element: EditorElement) => ({
          ...element,
          placement: {
            ...element.placement,
            x: element.placement.x * scaleX,
            y: element.placement.y * scaleY,
            width: element.placement.width * scaleX,
            height: element.placement.height * scaleY,
          },
        })
      );
    }

    this.canvas_width = width;
    this.canvas_height = height;
    this.root.elementStore.refreshElements();
  }

  setBackgroundColor(backgroundColor: string) {
    this.backgroundColor = backgroundColor;
    this.layer?.batchDraw();
  }

  batchDraw() {
    this.layer?.batchDraw();
  }
}
