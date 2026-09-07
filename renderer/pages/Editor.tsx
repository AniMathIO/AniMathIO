"use client";

import React, { useEffect, useRef, useCallback } from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import Resources from "./components/Resources";
import ElementsPanel from "./components/panels/ElementsPanel";
import Menu from "./Menu";
import Timeline from "./components/Timeline";
import Head from "next/head";
import AniMathIO from "../public/images/AniMathIO.png";
import dynamic from "next/dynamic";
import Konva from "konva";
import { Stage, Layer, Image, Text, Line, Transformer } from "react-konva";
import type { EditorElement, VideoEditorElement, ImageEditorElement, TextEditorElement, MafsEditorElement } from "@/types";
import {
  makeImageSceneFunc,
  getFilterFromEffectType,
  snapNodeToGuides,
  clampNodeToStage,
  NO_GUIDES,
  type SnapGuides,
} from "@/utils/konva-utils";
import { getContrastColor } from "@/utils/color";
import { classifyDroppedFile, computeDropCanvasPosition } from "@/utils/dragDropFiles";
import { waitForElementById, waitForMediaReady } from "@/utils/domLoad";

// ============================================================
// Individual element renderers
// ============================================================

/**
 * The <video>/<img> DOM node this hook resolves is rendered by a sibling
 * component (Media Pool) that can mount an arbitrary number of ticks after
 * this one, especially on a cold project load. A single post-commit effect
 * lookup can permanently miss it, so watch the DOM until it actually appears.
 */
function useDomElementById<T extends HTMLElement>(id: string): T | null {
  const [el, setEl] = React.useState<T | null>(null);

  useEffect(() => {
    setEl(null);
    const existing = document.getElementById(id) as T | null;
    if (existing) {
      setEl(existing);
      return;
    }
    const observer = new MutationObserver(() => {
      const found = document.getElementById(id) as T | null;
      if (found) {
        setEl(found);
        observer.disconnect();
      }
    });
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [id]);

  return el;
}

const VideoElementNode = observer(({ element, stateCtx, onGuidesChange }: { element: VideoEditorElement; stateCtx: any; onGuidesChange: (guides: SnapGuides) => void }) => {
  const nodeRef = useRef<Konva.Image>(null);
  const animRef = useRef<Konva.Animation | null>(null);
  const videoElement = useDomElementById<HTMLVideoElement>(element.properties.elementId);

  useEffect(() => {
    const node = nodeRef.current;
    if (!node) return;
    stateCtx.setKonvaNode(element.id, node);

    // Konva.Animation keeps re-drawing the layer so video frames update
    const layer = node.getLayer();
    if (layer) {
      animRef.current = new Konva.Animation(() => {}, layer);
      animRef.current.start();
    }
    return () => {
      animRef.current?.stop();
      stateCtx.setKonvaNode(element.id, null);
    };
  }, [videoElement]);

  if (!videoElement) return null;

  const { x, y, width, height, scaleX, scaleY, rotation } = element.placement;
  const effect = element.properties.effect?.type ?? "none";

  return (
    <Image
      ref={nodeRef}
      name="editorElement"
      image={videoElement}
      x={x}
      y={y}
      width={width}
      height={height}
      scaleX={scaleX}
      scaleY={scaleY}
      rotation={rotation}
      draggable
      dragBoundFunc={(pos) => clampNodeToStage(nodeRef.current!, pos, stateCtx.canvas_width, stateCtx.canvas_height)}
      sceneFunc={makeImageSceneFunc(() => videoElement, effect as any)}
      onClick={() => stateCtx.setSelectedElement(element)}
      onTap={() => stateCtx.setSelectedElement(element)}
      onDragMove={(e) => {
        const stage = e.target.getStage();
        if (stage) onGuidesChange(snapNodeToGuides(e.target, stage));
      }}
      onDragEnd={(e) => {
        onGuidesChange(NO_GUIDES);
        stateCtx.updateEditorElement({
          ...element,
          placement: { ...element.placement, x: e.target.x(), y: e.target.y() },
        });
      }}
      onTransformEnd={(e) => {
        stateCtx.updateEditorElement({
          ...element,
          placement: {
            ...element.placement,
            x: e.target.x(),
            y: e.target.y(),
            scaleX: e.target.scaleX(),
            scaleY: e.target.scaleY(),
            rotation: e.target.rotation(),
          },
        });
      }}
    />
  );
});

const ImageElementNode = observer(({ element, stateCtx, onGuidesChange }: { element: ImageEditorElement | MafsEditorElement; stateCtx: any; onGuidesChange: (guides: SnapGuides) => void }) => {
  const nodeRef = useRef<Konva.Image>(null);
  const imgElement = useDomElementById<HTMLImageElement>(element.properties.elementId);

  useEffect(() => {
    const node = nodeRef.current;
    if (!node) return;
    stateCtx.setKonvaNode(element.id, node);
    // The node attaches to the layer after any earlier seek/play batchDraw already
    // ran, so force one redraw now or it stays attached-but-unpainted.
    node.getLayer()?.batchDraw();
    return () => stateCtx.setKonvaNode(element.id, null);
  }, [imgElement]);

  if (!imgElement) return null;

  const { x, y, width, height, scaleX, scaleY, rotation } = element.placement;
  const effect = element.properties.effect?.type ?? "none";

  return (
    <Image
      ref={nodeRef}
      name="editorElement"
      image={imgElement}
      x={x}
      y={y}
      width={width}
      height={height}
      scaleX={scaleX}
      scaleY={scaleY}
      rotation={rotation}
      draggable
      dragBoundFunc={(pos) => clampNodeToStage(nodeRef.current!, pos, stateCtx.canvas_width, stateCtx.canvas_height)}
      sceneFunc={makeImageSceneFunc(() => imgElement, effect as any)}
      onClick={() => stateCtx.setSelectedElement(element)}
      onTap={() => stateCtx.setSelectedElement(element)}
      onDragMove={(e) => {
        const stage = e.target.getStage();
        if (stage) onGuidesChange(snapNodeToGuides(e.target, stage));
      }}
      onDragEnd={(e) => {
        onGuidesChange(NO_GUIDES);
        stateCtx.updateEditorElement({
          ...element,
          placement: { ...element.placement, x: e.target.x(), y: e.target.y() },
        });
      }}
      onTransformEnd={(e) => {
        stateCtx.updateEditorElement({
          ...element,
          placement: {
            ...element.placement,
            x: e.target.x(),
            y: e.target.y(),
            scaleX: e.target.scaleX(),
            scaleY: e.target.scaleY(),
            rotation: e.target.rotation(),
          },
        });
      }}
    />
  );
});

const TextElementNode = observer(({ element, stateCtx, onGuidesChange }: { element: TextEditorElement; stateCtx: any; onGuidesChange: (guides: SnapGuides) => void }) => {
  const nodeRef = useRef<Konva.Text>(null);

  useEffect(() => {
    const node = nodeRef.current;
    if (!node) return;
    stateCtx.setKonvaNode(element.id, node);
    return () => stateCtx.setKonvaNode(element.id, null);
  }, []);

  const { x, y, width, height, scaleX, scaleY, rotation } = element.placement;

  return (
    <Text
      ref={nodeRef}
      name="editorElement"
      text={element.properties.text}
      x={x}
      y={y}
      width={width}
      height={height}
      scaleX={scaleX}
      scaleY={scaleY}
      rotation={rotation}
      fontSize={element.properties.fontSize}
      fontStyle={String(element.properties.fontWeight)}
      fill={element.properties.color ?? getContrastColor(stateCtx.backgroundColor)}
      draggable
      dragBoundFunc={(pos) => clampNodeToStage(nodeRef.current!, pos, stateCtx.canvas_width, stateCtx.canvas_height)}
      onClick={() => stateCtx.setSelectedElement(element)}
      onTap={() => stateCtx.setSelectedElement(element)}
      onDragMove={(e) => {
        const stage = e.target.getStage();
        if (stage) onGuidesChange(snapNodeToGuides(e.target, stage));
      }}
      onDragEnd={(e) => {
        onGuidesChange(NO_GUIDES);
        stateCtx.updateEditorElement({
          ...element,
          placement: { ...element.placement, x: e.target.x(), y: e.target.y() },
        });
      }}
      onTransformEnd={(e) => {
        stateCtx.updateEditorElement({
          ...element,
          placement: {
            ...element.placement,
            x: e.target.x(),
            y: e.target.y(),
            scaleX: e.target.scaleX(),
            scaleY: e.target.scaleY(),
            rotation: e.target.rotation(),
          },
        });
      }}
    />
  );
});

const EditorElementNode = observer(({ element, stateCtx, onGuidesChange }: { element: EditorElement; stateCtx: any; onGuidesChange: (guides: SnapGuides) => void }) => {
  switch (element.type) {
    case "video":
      return <VideoElementNode element={element} stateCtx={stateCtx} onGuidesChange={onGuidesChange} />;
    case "image":
    case "mafs":
      return <ImageElementNode element={element} stateCtx={stateCtx} onGuidesChange={onGuidesChange} />;
    case "text":
      return <TextElementNode element={element} stateCtx={stateCtx} onGuidesChange={onGuidesChange} />;
    case "audio":
      return null; // Audio has no visual representation on the canvas
    default:
      return null;
  }
});

// ============================================================
// Main EditorCanvas component
// ============================================================

const EditorCanvas = observer(() => {
  const state = React.useContext(StateContext);
  const stageRef = useRef<Konva.Stage>(null);
  const layerRef = useRef<Konva.Layer>(null);
  const transformerRef = useRef<Konva.Transformer>(null);
  const [guides, setGuides] = React.useState<SnapGuides>(NO_GUIDES);

  // Register stage with state
  useEffect(() => {
    if (!state.isEditorActive) return;
    if (!stageRef.current || !layerRef.current) return;
    state.setStage(stageRef.current, layerRef.current, state.canvas_width, state.canvas_height);
  }, [state.isEditorActive]);

  // Update transformer when selection changes
  useEffect(() => {
    if (!transformerRef.current || !layerRef.current) return;
    const selectedElement = state.selectedElement;
    if (selectedElement && selectedElement.type !== "audio") {
      const konvaNode = state.getKonvaNode(selectedElement.id);
      if (konvaNode) {
        transformerRef.current.nodes([konvaNode]);
      } else {
        transformerRef.current.nodes([]);
      }
    } else {
      transformerRef.current.nodes([]);
    }
    layerRef.current.batchDraw();
  }, [state.selectedElement]);

  // Deselect when clicking empty stage area
  const handleStageClick = useCallback(
    (e: Konva.KonvaEventObject<MouseEvent>) => {
      if (e.target === stageRef.current) {
        state.setSelectedElement(null);
      }
    },
    [state]
  );

  return (
    <Stage
      ref={stageRef}
      width={state.canvas_width}
      height={state.canvas_height}
      style={{ backgroundColor: state.backgroundColor }}
      onClick={handleStageClick}
      onTap={handleStageClick as any}
    >
      <Layer ref={layerRef}>
        {state.editorElements.map((element) => (
          <EditorElementNode key={element.id} element={element} stateCtx={state} onGuidesChange={setGuides} />
        ))}
        <Transformer
          ref={transformerRef}
          borderStroke="#00a0f5"
          borderStrokeWidth={2}
          anchorStroke="#0063d8"
          anchorFill="#ffffff"
          anchorSize={10}
          anchorCornerRadius={5}
          keepRatio={false}
        />
      </Layer>
      <Layer listening={false}>
        {guides.vertical.map((x, i) => (
          <Line key={`v-${i}`} points={[x, 0, x, state.canvas_height]} stroke="#ff0000" strokeWidth={1} dash={[4, 4]} />
        ))}
        {guides.horizontal.map((y, i) => (
          <Line key={`h-${i}`} points={[0, y, state.canvas_width, y]} stroke="#ff0000" strokeWidth={1} dash={[4, 4]} />
        ))}
      </Layer>
    </Stage>
  );
});

// ============================================================
// Full Editor layout
// ============================================================

const canvasScaleMap: Record<string, { max: number; default: number }> = {
  "640x360": { max: 100, default: 100 },
  "800x600": { max: 70, default: 70 },
  "854x480": { max: 85, default: 85 },
  "720x1280": { max: 38, default: 38 },
  "1080x1920": { max: 25, default: 25 },
  "1080x1080": { max: 40, default: 40 },
  "1280x720": { max: 55, default: 55 },
  "1920x1080": { max: 37, default: 37 },
};

const EditorInner = observer(() => {
  const state = React.useContext(StateContext);
  const [scaleFactor, setScaleFactor] = React.useState(25);
  const canvasWrapperRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!state.isEditorActive) return;
    const defaultScaleFactor =
      canvasScaleMap[`${state.canvas_width}x${state.canvas_height}`]?.default || 37;
    setScaleFactor(defaultScaleFactor);
  }, [state.canvas_width, state.canvas_height, state.isEditorActive]);

  // Global keyboard shortcuts (arrow-key seek/nudge, delete, copy/paste,
  // space to play/pause) are only meaningful while the editor is mounted -
  // the Dashboard shares this same store and must not react to them.
  useEffect(() => {
    state.attachKeyboardShortcuts();
    return () => {
      state.detachKeyboardShortcuts();
    };
  }, [state]);

  if (!state.isEditorActive) return null;

  const handleScaleChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const newScaleFactor = parseInt(event.target.value);
    const key = `${state.canvas_width}x${state.canvas_height}`;
    const maxScaleFactor = canvasScaleMap[key]?.max || 100;
    setScaleFactor(Math.min(newScaleFactor, maxScaleFactor));
  };

  const handleCanvasDragOver = (e: React.DragEvent<HTMLDivElement>) => {
    if (e.dataTransfer.types.includes("application/x-animathio-resource")) {
      e.preventDefault();
    }
  };

  const handleCanvasDrop = (e: React.DragEvent<HTMLDivElement>) => {
    const payload = e.dataTransfer.getData("application/x-animathio-resource");
    if (!payload) return;
    e.preventDefault();

    let resource: { kind: string; index?: number; text?: string; fontSize?: number; fontWeight?: number };
    try {
      resource = JSON.parse(payload);
    } catch {
      return;
    }

    // The Canvas Scale slider is a pure CSS transform (see the wrapper div's
    // style below), so this element's own bounding rect already reflects the
    // scaled visual size — dividing it out here converts the drop point back
    // into unscaled Konva stage coordinates without needing scaleFactor math.
    const rect = e.currentTarget.getBoundingClientRect();
    const dropPosition = {
      x: ((e.clientX - rect.left) / rect.width) * state.canvas_width,
      y: ((e.clientY - rect.top) / rect.height) * state.canvas_height,
    };

    switch (resource.kind) {
      case "video":
        if (resource.index !== undefined) state.addVideo(resource.index, dropPosition);
        break;
      case "image":
        if (resource.index !== undefined) state.addImage(resource.index, dropPosition);
        break;
      case "text":
        if (resource.text !== undefined && resource.fontSize !== undefined && resource.fontWeight !== undefined) {
          state.addText({ text: resource.text, fontSize: resource.fontSize, fontWeight: resource.fontWeight }, dropPosition);
        }
        break;
    }
  };

  // OS-level file drag-in (Finder/Explorer/another app) is independent of the
  // app's own application/x-animathio-resource drags handled above — a native
  // file drag never sets that MIME type, so the two coexist without conflict.
  // "Files" is the standard dataTransfer.types signal available during
  // dragover, before the actual file list is populated at drop time.
  const handleAppDragOver = (e: React.DragEvent<HTMLDivElement>) => {
    if (e.dataTransfer.types.includes("Files")) {
      e.preventDefault();
    }
  };

  const handleAppDrop = (e: React.DragEvent<HTMLDivElement>) => {
    const files = Array.from(e.dataTransfer.files);
    if (files.length === 0) return;
    e.preventDefault();

    // Capture synthetic-event-derived values now, before any await.
    const clientX = e.clientX;
    const clientY = e.clientY;
    const canvasRect = canvasWrapperRef.current?.getBoundingClientRect() ?? null;
    const dropPosition = computeDropCanvasPosition(clientX, clientY, canvasRect, state.canvas_width, state.canvas_height);

    const addDroppedFiles = async () => {
      for (const file of files) {
        const kind = classifyDroppedFile(file);
        if (!kind) {
          console.warn(`Dropped file "${file.name}" has an unsupported type (${file.type || "unknown"}); skipping.`);
          continue;
        }

        const url = URL.createObjectURL(file);

        if (kind === "video") {
          const index = state.videos.length;
          state.addVideoResource(url);
          state.setSelectedMenuOption("Videos");
          const el = await waitForElementById(`video-${index}`);
          if (el) {
            await waitForMediaReady(el as HTMLVideoElement);
            state.addVideo(index, dropPosition);
          }
        } else if (kind === "image") {
          const index = state.images.length;
          state.addImageResource(url);
          state.setSelectedMenuOption("Images");
          const el = await waitForElementById(`image-${index}`);
          if (el) {
            await waitForMediaReady(el as HTMLImageElement);
            state.addImage(index, dropPosition);
          }
        } else {
          const index = state.audios.length;
          state.addAudioResource(url);
          state.setSelectedMenuOption("Audios");
          const el = await waitForElementById(`audio-${index}`);
          if (el) {
            await waitForMediaReady(el as HTMLAudioElement);
            state.addAudio(index);
          }
        }
      }
    };

    addDroppedFiles();
  };

  return (
    <React.Fragment>
      <Head>
        <title>AniMathIO - Editor</title>
        <link rel="icon" href={AniMathIO.src} />
      </Head>

      <div
        className="bg-slate-200 dark:bg-gray-800 grid grid-rows-[500px_1fr_20px] grid-cols-[90px_300px_250px_1fr] h-[calc(100svh-32px)]"
        onDragOver={handleAppDragOver}
        onDrop={handleAppDrop}
      >
        <div className="tile row-span-2 flex flex-col">
          <Menu />
        </div>
        <div className="row-span-2 flex flex-col overflow-scroll">
          <Resources />
        </div>
        <div className="col-start-3 row-start-1">
          <ElementsPanel />
        </div>

        <div
          id="grid-canvas-container"
          className="col-start-4 bg-gray-200 dark:bg-gray-700 dark:text-white grid w-[900px] h-[500px] place-self-center place-content-center"
        >
          <div
            ref={canvasWrapperRef}
            style={{
              transformOrigin: "center",
              transform: `scale(${scaleFactor / 100})`,
            }}
            className="flex w-fit h-fit"
            onDragOver={handleCanvasDragOver}
            onDrop={handleCanvasDrop}
          >
            <EditorCanvas />
          </div>
          <div className="mt-4 absolute justify-self-start self-end p-2">
            <p>Canvas Scale:</p>
            <input
              title="Canvas Scale"
              type="range"
              min="25"
              max={canvasScaleMap[`${state.canvas_width}x${state.canvas_height}`]?.max || 100}
              value={scaleFactor}
              onChange={handleScaleChange}
              className="w-48 bg-gray-700 dark:bg-gray-900 rounded-lg appearance-none cursor-pointer"
            />
            <span className="ml-2">{scaleFactor}%</span>
          </div>
        </div>

        <div className="col-start-3 row-start-2 col-span-2 relative px-[10px] py-[4px] overflow-scroll">
          <Timeline />
        </div>
      </div>
    </React.Fragment>
  );
});

// Use dynamic import with ssr:false since Konva requires browser APIs
const Editor = dynamic(() => Promise.resolve(EditorInner), { ssr: false });

export default Editor;
