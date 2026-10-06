import { getUid } from "@/utils";
import { getContrastColor } from "@/utils/color";
import { renderLatexToImage } from "@/utils/katex-render";
import { parseManimScene } from "@/utils/manim/parser";
import { translateManimScene } from "@/utils/manim/translator";
import type { Animation, EditorElement, Placement, TimeFrame } from "@/types";
import type { RootStore } from "@/states/RootStore";
import { waitForElementById, waitForMediaReady } from "@/utils/domLoad";

type Args = Record<string, any>;

/** Thrown for problems the agent can act on; surfaced as the tool's error text. */
class ToolError extends Error {}

const ANIMATION_TYPES = [
  "fadeIn",
  "fadeOut",
  "slideIn",
  "slideOut",
  "breathe",
  "mafsReveal",
] as const;

function requireEditor(state: RootStore) {
  if (!state.isEditorActive) {
    throw new ToolError(
      "No project is open in AniMathIO. Create or open one before using this tool."
    );
  }
}

function requireString(args: Args, key: string): string {
  const value = args[key];
  if (typeof value !== "string" || !value.trim()) {
    throw new ToolError(`"${key}" is required and must be a non-empty string.`);
  }
  return value;
}

function requireNumber(args: Args, key: string): number {
  const value = args[key];
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new ToolError(`"${key}" is required and must be a number.`);
  }
  return value;
}

function findElement(state: RootStore, id: string): EditorElement {
  const element = state.editorElements.find((e) => e.id === id);
  if (!element) {
    throw new ToolError(
      `No element with id "${id}". Call get_project_state to list current element ids.`
    );
  }
  return element;
}

/** Centres a box of the given size, which is what omitting x/y means. */
function centredPlacement(
  state: RootStore,
  width: number,
  height: number,
  args: Args
): Placement {
  const x = typeof args.x === "number" ? args.x : (state.canvas_width - width) / 2;
  const y = typeof args.y === "number" ? args.y : (state.canvas_height - height) / 2;
  return { x, y, width, height, rotation: 0, scaleX: 1, scaleY: 1 };
}

function fullTimeFrame(state: RootStore): TimeFrame {
  return { start: 0, end: state.maxTime };
}

function describeElement(element: EditorElement) {
  return {
    id: element.id,
    name: element.name,
    type: element.type,
    // MobX objects are proxies, which Electron's structured clone rejects.
    placement: { ...element.placement },
    timeFrame: { ...element.timeFrame },
  };
}

/**
 * Every tool call from the MCP server lands here and is applied to the live
 * RootStore, so the canvas updates as the agent works. Each handler returns
 * plain JSON-serialisable data — it crosses an IPC boundary.
 */
export function createMcpDispatcher(state: RootStore) {
  let importingMedia = false;
  const handlers: Record<string, (args: Args) => Promise<unknown>> = {
    async add_media(args) {
      requireEditor(state);
      const path = requireString(args, "path");
      if (importingMedia) throw new ToolError("Another media import is in progress. Wait for it to finish.");
      importingMedia = true;
      try {
        const result = await window.electron.readMediaFile(path);
        if (!result.success || !result.type || !result.dataUrl) {
          throw new ToolError(result.error ?? `Could not read media file "${path}".`);
        }
        const { type, dataUrl } = result;
        const index = type === "video" ? state.videos.length : type === "image" ? state.images.length : state.audios.length;
        if (type === "video") { state.addVideoResource(dataUrl); state.selectedMenuOption = "Videos"; }
        else if (type === "image") { state.addImageResource(dataUrl); state.selectedMenuOption = "Images"; }
        else { state.addAudioResource(dataUrl); state.selectedMenuOption = "Audios"; }

        const element = await waitForElementById(`${type}-${index}`);
        if (!element) throw new ToolError(`The ${type} resource element never mounted. Keep the editor and resource panel open.`);
        let timer: ReturnType<typeof setTimeout> | undefined;
        try {
          await Promise.race([
            waitForMediaReady(element as HTMLVideoElement | HTMLImageElement | HTMLAudioElement),
            new Promise<never>((_resolve, reject) => {
              timer = setTimeout(() => reject(new ToolError(`Timed out decoding media file "${path}".`)), 10_000);
            }),
          ]);
        } finally { clearTimeout(timer); }
        if (type === "image") {
          const image = element as HTMLImageElement;
          if (!(image.naturalWidth > 0 && image.naturalHeight > 0)) throw new ToolError(`Could not decode image "${path}".`);
        } else {
          const media = element as HTMLMediaElement;
          if (media.error || !Number.isFinite(media.duration) || media.duration <= 0) throw new ToolError(`Could not decode ${type} "${path}".`);
          if (type === "video") {
            const video = element as HTMLVideoElement;
            if (!(video.videoWidth > 0 && video.videoHeight > 0)) throw new ToolError(`Could not decode video "${path}".`);
          }
        }
        requireEditor(state);
        const before = new Set(state.editorElements.map(e => e.id));
        if (type === "video") state.addVideo(index);
        else if (type === "image") state.addImage(index);
        else state.addAudio(index);
        const ids = state.editorElements.filter(e => !before.has(e.id)).map(e => e.id);
        if (!ids.length) throw new ToolError(`The ${type} timeline element could not be created.`);
        return { ids, type };
      } finally { importingMedia = false; }
    },

    async export_video(args) {
      requireEditor(state);
      const path = requireString(args, "path");
      if (!path.startsWith("/") && !/^[A-Za-z]:[\\/]/.test(path) && !path.startsWith("\\\\")) {
        throw new ToolError("An absolute destination path is required.");
      }
      const extension = /\.(mp4|webm)$/i.exec(path)?.[1].toLowerCase();
      if (!extension) throw new ToolError("Video destination must end in .mp4 or .webm.");
      const format = args.format ?? extension;
      if (format !== "mp4" && format !== "webm") throw new ToolError('"format" must be mp4 or webm.');
      if (format !== extension) throw new ToolError("Export format must match the destination extension.");
      await state.exportStore.saveCanvasToVideoToPath(path, format);
      return { exported: true, path, format };
    },
    async get_project_state() {
      return {
        isEditorActive: state.isEditorActive,
        canvas: {
          width: state.canvas_width,
          height: state.canvas_height,
          backgroundColor: state.backgroundColor,
        },
        maxTimeMs: state.maxTime,
        currentTimeMs: state.currentTimeInMs,
        elements: state.editorElements.map(describeElement),
        animations: state.animations.map((a) => ({
          id: a.id,
          targetId: a.targetId,
          type: a.type,
          durationMs: a.duration,
        })),
      };
    },

    async add_text(args) {
      requireEditor(state);
      const text = requireString(args, "text");
      const fontSize = typeof args.fontSize === "number" ? args.fontSize : 32;
      const fontWeight = typeof args.fontWeight === "number" ? args.fontWeight : 400;

      const before = new Set(state.editorElements.map((e) => e.id));
      state.addText(
        { text, fontSize, fontWeight },
        typeof args.x === "number" && typeof args.y === "number"
          ? { x: args.x, y: args.y }
          : undefined
      );

      const created = state.editorElements.find((e) => !before.has(e.id));
      if (!created) throw new ToolError("The text element could not be created.");

      if (typeof args.color === "string") {
        await state.updateEditorElement({
          ...created,
          properties: { ...created.properties, color: args.color },
        } as EditorElement);
      }
      return { id: created.id };
    },

    async add_math(args) {
      requireEditor(state);
      const latex = requireString(args, "latex");

      let rendered: { dataUrl: string; width: number; height: number };
      try {
        rendered = await renderLatexToImage(latex, {
          color:
            typeof args.color === "string"
              ? args.color
              : getContrastColor(state.backgroundColor),
        });
      } catch (error) {
        throw new ToolError(
          `"${latex}" is not valid LaTeX: ${error instanceof Error ? error.message : String(error)}`
        );
      }

      const id = getUid();
      const element = {
        id,
        name: `Math ${latex.slice(0, 24)}`,
        type: "mafs",
        placement: centredPlacement(state, rendered.width, rendered.height, args),
        timeFrame: fullTimeFrame(state),
        properties: {
          elementId: `mcp-math-${id}`,
          src: rendered.dataUrl,
          effect: { type: "none" },
        },
      } as unknown as EditorElement;

      await state.addEditorElement(element);
      return { id };
    },

    async import_manim_scene(args) {
      requireEditor(state);
      const script = requireString(args, "script");

      const parsed = parseManimScene(script);
      const result = await translateManimScene(
        parsed,
        { width: state.canvas_width, height: state.canvas_height },
        state.backgroundColor
      );

      if (result.durationMs > state.maxTime) state.setMaxTime(result.durationMs);
      for (const element of result.elements) await state.addEditorElement(element);
      for (const animation of result.animations) state.addAnimation(animation);

      return {
        elementsCreated: result.elements.length,
        animationsCreated: result.animations.length,
        durationMs: result.durationMs,
        warnings: result.warnings.map((w) => w.message),
      };
    },

    async add_animation(args) {
      requireEditor(state);
      const targetId = requireString(args, "targetId");
      const type = requireString(args, "type");
      const durationMs = requireNumber(args, "durationMs");

      if (!(ANIMATION_TYPES as readonly string[]).includes(type)) {
        throw new ToolError(
          `"${type}" is not a known animation. Use one of: ${ANIMATION_TYPES.join(", ")}.`
        );
      }
      findElement(state, targetId);

      const id = getUid();
      const needsDirection = type === "slideIn" || type === "slideOut";
      const animation = {
        id,
        targetId,
        duration: durationMs,
        type,
        properties: needsDirection
          ? { direction: typeof args.direction === "string" ? args.direction : "left" }
          : {},
      } as unknown as Animation;

      state.addAnimation(animation);
      return { id };
    },

    async update_element(args) {
      requireEditor(state);
      const id = requireString(args, "id");
      const element = findElement(state, id);

      const next = {
        ...element,
        placement: { ...element.placement, ...(args.placement ?? {}) },
        timeFrame: { ...element.timeFrame, ...(args.timeFrame ?? {}) },
      } as EditorElement;

      await state.updateEditorElement(next);
      return { id };
    },

    async remove_element(args) {
      requireEditor(state);
      const id = requireString(args, "id");
      findElement(state, id);
      state.removeEditorElement(id);
      return { removed: true };
    },

    async set_canvas(args) {
      requireEditor(state);
      if (typeof args.width === "number" || typeof args.height === "number") {
        state.setCanvasSize(
          typeof args.width === "number" ? args.width : state.canvas_width,
          typeof args.height === "number" ? args.height : state.canvas_height
        );
      }
      if (typeof args.backgroundColor === "string") {
        state.setBackgroundColor(args.backgroundColor);
      }
      if (typeof args.maxTimeMs === "number") state.setMaxTime(args.maxTimeMs);

      return {
        canvas: {
          width: state.canvas_width,
          height: state.canvas_height,
          backgroundColor: state.backgroundColor,
        },
        maxTimeMs: state.maxTime,
      };
    },

    async seek(args) {
      requireEditor(state);
      const timeMs = requireNumber(args, "timeMs");
      state.handleSeek(Math.max(0, Math.min(timeMs, state.maxTime)));
      return { currentTimeMs: state.currentTimeInMs };
    },

    async set_playing(args) {
      requireEditor(state);
      if (typeof args.playing !== "boolean") {
        throw new ToolError('"playing" is required and must be a boolean.');
      }
      state.setPlaying(args.playing);
      return { playing: state.playing };
    },

    async save_project(args) {
      requireEditor(state);
      const path =
        typeof args.path === "string" && args.path ? args.path : state.currentProjectFilePath;
      if (!path) {
        throw new ToolError(
          "This project has never been saved, so there is no path to save to. Pass an explicit \"path\"."
        );
      }

      const data = await state.serialize();
      const bytes = Array.from(new Uint8Array(data as unknown as ArrayBuffer));
      const result = await window.electron.writeProjectFile(path, bytes);
      if (!result?.success) {
        throw new ToolError(`Could not write the project to "${path}".`);
      }
      return { saved: true, path };
    },
  };

  return async function dispatch(tool: string, args: Args): Promise<unknown> {
    const handler = handlers[tool];
    if (!handler) throw new ToolError(`Unknown tool "${tool}".`);
    return handler(args ?? {});
  };
}
