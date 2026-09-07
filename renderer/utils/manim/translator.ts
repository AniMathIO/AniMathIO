/**
 * Translates a parsed ManimScene into AniMathIO EditorElements and Animations.
 *
 * Mapping strategy:
 *  - Text  →  TextEditorElement (editable Konva text)
 *  - Tex / MathTex  →  MafsEditorElement (KaTeX-rendered raster image; Konva's
 *    canvas Text node can't typeset math directly)
 *  - Circle / Rectangle / Arrow  →  TextEditorElement (SVG-like placeholder with label)
 *  - ImageMobject  →  ImageEditorElement (src must be resolved externally)
 *  - Create / Write / GrowFromCenter  →  FadeIn animation
 *  - FadeIn  →  FadeIn animation
 *  - FadeOut  →  FadeOut animation
 *  - Indicate / Other  →  Breathe animation (visual pulse)
 *  - self.wait(n)  →  advances the timeline cursor by n seconds
 *
 * Canvas assumptions: 1920 × 1080 Manim frame maps to the AniMathIO canvas.
 * Manim uses a coordinate system centred at (0,0) with x ∈ [-7,7], y ∈ [-4,4].
 */

import { getUid } from "@/utils";
import { renderLatexToImage } from "@/utils/katex-render";
import { getContrastColor } from "@/utils/color";
import type {
  EditorElement,
  TextEditorElement,
  ImageEditorElement,
  MafsEditorElement,
  Animation,
  Placement,
  TimeFrame,
} from "@/types";
import type {
  ManimScene,
  ManimMobject,
  ManimTex,
  ManimPlayAnimation,
  ManimTranslationResult,
  ManimTranslationWarning,
} from "./types";

// ---------- constants ----------

/** Default animation run time in seconds when not specified */
const DEFAULT_RUN_TIME = 1;
/** Default pause added after each self.play() if no explicit wait follows */
const DEFAULT_POST_PLAY_PAUSE = 0;
/** Manim canvas half-widths */
const MANIM_HW = 7;
const MANIM_HH = 4;

// ---------- coordinate helpers ----------

interface CanvasSize {
  width: number;
  height: number;
}

/** Convert Manim [x, y, z] to AniMathIO pixel coords (top-left origin) */
function manimToPixel(
  mx: number,
  my: number,
  canvas: CanvasSize
): { x: number; y: number } {
  return {
    x: ((mx + MANIM_HW) / (2 * MANIM_HW)) * canvas.width,
    y: ((MANIM_HH - my) / (2 * MANIM_HH)) * canvas.height,
  };
}

function defaultPlacement(canvas: CanvasSize, index: number): Placement {
  const cols = 3;
  const col = index % cols;
  const row = Math.floor(index / cols);
  const cellW = canvas.width / cols;
  const cellH = canvas.height / 3;
  return {
    x: col * cellW + cellW * 0.1,
    y: row * cellH + cellH * 0.25,
    width: cellW * 0.8,
    height: cellH * 0.5,
    rotation: 0,
    scaleX: 1,
    scaleY: 1,
  };
}

// ---------- color conversion ----------

const MANIM_NAMED_COLORS: Record<string, string> = {
  WHITE: "#FFFFFF",
  BLACK: "#000000",
  RED: "#FF0000",
  GREEN: "#00FF00",
  BLUE: "#0000FF",
  YELLOW: "#FFFF00",
  ORANGE: "#FF8800",
  PURPLE: "#AA00FF",
  PINK: "#FF69B4",
  TEAL: "#00AAAA",
  GREY: "#888888",
  GRAY: "#888888",
  RED_E: "#CF3E7A",
  GREEN_E: "#4CAF50",
  BLUE_E: "#1E90FF",
  GOLD: "#FFD700",
  MAROON: "#800000",
};

/** Resolves a Manim color name/hex to a hex string, or undefined if none was given. */
function resolveColor(color: string | undefined): string | undefined {
  if (!color) return undefined;
  if (color.startsWith("#")) return color;
  return MANIM_NAMED_COLORS[color.toUpperCase()] ?? "#FFFFFF";
}

// ---------- element builders ----------

function buildTextElement(
  mob: Extract<ManimMobject, { kind: "Text" }>,
  placement: Placement,
  timeFrame: TimeFrame
): TextEditorElement {
  return {
    id: getUid(),
    name: mob.id,
    type: "text",
    placement,
    timeFrame,
    properties: {
      text: mob.text,
      fontSize: mob.fontSize ?? 36,
      fontWeight: mob.fontWeight ?? 400,
      splittedTexts: [],
      // Only set when the script specifies color=; otherwise left undefined so
      // TextElementNode falls back to contrasting against the canvas background.
      color: resolveColor(mob.color),
    },
  };
}

/** Scales natural image dimensions to fit within a cell, preserving aspect ratio. */
function fitWithinCell(
  naturalWidth: number,
  naturalHeight: number,
  cell: { width: number; height: number }
): { width: number; height: number } {
  if (naturalWidth <= 0 || naturalHeight <= 0) {
    return { width: cell.width, height: cell.height };
  }
  const scale = Math.min(cell.width / naturalWidth, cell.height / naturalHeight, 1);
  return { width: naturalWidth * scale, height: naturalHeight * scale };
}

async function buildMathTexElement(
  mob: ManimTex,
  placement: Placement,
  timeFrame: TimeFrame,
  backgroundColor: string
): Promise<MafsEditorElement> {
  const { dataUrl, width, height } = await renderLatexToImage(mob.tex, {
    color: resolveColor(mob.color) ?? getContrastColor(backgroundColor),
  });
  const fitted = fitWithinCell(width, height, placement);
  const id = getUid();

  return {
    id,
    name: mob.id,
    type: "mafs",
    placement: { ...placement, width: fitted.width, height: fitted.height },
    timeFrame,
    properties: {
      elementId: `manim-tex-${mob.id}`,
      src: dataUrl,
      effect: { type: "none" },
    },
  };
}

/** Fallback when KaTeX rendering fails: plain text showing the raw LaTeX source. */
function buildMathTexFallbackElement(
  mob: ManimTex,
  placement: Placement,
  timeFrame: TimeFrame
): TextEditorElement {
  return {
    id: getUid(),
    name: mob.id,
    type: "text",
    placement,
    timeFrame,
    properties: {
      text: `$${mob.tex}$`,
      fontSize: 36,
      fontWeight: 400,
      splittedTexts: [],
    },
  };
}

function buildShapeElement(
  mob: Extract<ManimMobject, { kind: "Circle" | "Rectangle" | "Arrow" }>,
  placement: Placement,
  timeFrame: TimeFrame
): TextEditorElement {
  const label =
    mob.kind === "Circle"
      ? `⬤ Circle`
      : mob.kind === "Rectangle"
      ? `▭ Rectangle`
      : `→ Arrow`;

  return {
    id: getUid(),
    name: mob.id,
    type: "text",
    placement,
    timeFrame,
    properties: {
      text: label,
      fontSize: 28,
      fontWeight: 400,
      splittedTexts: [],
    },
  };
}

function buildImageElement(
  mob: Extract<ManimMobject, { kind: "ImageMobject" }>,
  placement: Placement,
  timeFrame: TimeFrame
): ImageEditorElement {
  return {
    id: getUid(),
    name: mob.id,
    type: "image",
    placement,
    timeFrame,
    properties: {
      src: mob.filename,
      elementId: `manim-img-${mob.id}`,
      effect: { type: "none" },
    },
  };
}

// ---------- animation builders ----------

function buildFadeInAnimation(targetId: string, startMs: number, durationMs: number): Animation {
  return {
    id: getUid(),
    targetId,
    duration: durationMs,
    type: "fadeIn",
    properties: {},
  };
}

function buildFadeOutAnimation(targetId: string, startMs: number, durationMs: number): Animation {
  return {
    id: getUid(),
    targetId,
    duration: durationMs,
    type: "fadeOut",
    properties: {},
  };
}

function buildBreatheAnimation(targetId: string, durationMs: number): Animation {
  return {
    id: getUid(),
    targetId,
    duration: durationMs,
    type: "breathe",
    properties: {},
  };
}

// ---------- main translator ----------

export async function translateManimScene(
  scene: ManimScene,
  canvas: CanvasSize = { width: 1920, height: 1080 },
  backgroundColor: string = "#111111"
): Promise<ManimTranslationResult> {
  const warnings: ManimTranslationWarning[] = [];
  const elements: EditorElement[] = [];
  const animations: Animation[] = [];

  // Maps manim variable name → editor element id
  const elementIdByVar = new Map<string, string>();
  // Maps manim variable name → Placement (for position-aware mobs)
  const placementByVar = new Map<string, Placement>();

  // Assign default placements based on declaration order
  const varNames = Object.keys(scene.mobjects);
  varNames.forEach((varName, idx) => {
    placementByVar.set(varName, defaultPlacement(canvas, idx));
  });

  // Cursor advances as we process statements
  let cursorMs = 0;
  const totalDurationMs = computeTotalDuration(scene);

  // All elements live for the whole timeline by default; we'll refine per animation
  // For simplicity each element's timeFrame = [firstAppearance, totalDuration]
  // We do a two-pass approach: first build elements, then process statements for animations

  // Pass 1: create element stubs with full-duration timeframes
  for (const varName of varNames) {
    const mob = scene.mobjects[varName];
    const placement = placementByVar.get(varName)!;
    const timeFrame: TimeFrame = { start: 0, end: totalDurationMs };

    let el: EditorElement | null = null;
    if (mob.kind === "Text") {
      el = buildTextElement(mob, placement, timeFrame);
    } else if (mob.kind === "Tex" || mob.kind === "MathTex") {
      try {
        el = await buildMathTexElement(mob, placement, timeFrame, backgroundColor);
      } catch (error) {
        warnings.push({
          message: `Failed to render KaTeX for "${varName}" ("${mob.tex}"): ${error instanceof Error ? error.message : String(error)}. Falling back to plain text.`,
        });
        el = buildMathTexFallbackElement(mob, placement, timeFrame);
      }
    } else if (mob.kind === "Circle" || mob.kind === "Rectangle" || mob.kind === "Arrow") {
      el = buildShapeElement(mob as Extract<ManimMobject, { kind: "Circle" | "Rectangle" | "Arrow" }>, placement, timeFrame);
    } else if (mob.kind === "ImageMobject") {
      el = buildImageElement(mob as Extract<ManimMobject, { kind: "ImageMobject" }>, placement, timeFrame);
      warnings.push({
        message: `ImageMobject "${varName}" references "${(mob as Extract<ManimMobject, { kind: "ImageMobject" }>).filename}", which AniMathIO can't resolve automatically — replace its source manually in the editor.`,
      });
    } else if (mob.kind === "VGroup" || mob.kind === "Group") {
      // Groups are skipped; animations on them target children instead
      warnings.push({ message: `VGroup/Group "${varName}" is not directly rendered; animate its children.` });
      continue;
    }

    if (el) {
      elements.push(el);
      elementIdByVar.set(varName, el.id);
    }
  }

  // Pass 2: process statements to generate animations and refine timeframes
  cursorMs = 0;

  // Track when each element first appears (for FadeIn) and disappears (for FadeOut)
  const firstAppearMs = new Map<string, number>();

  for (const stmt of scene.statements) {
    if (stmt.kind === "wait") {
      cursorMs += stmt.duration * 1000;
      continue;
    }

    // self.play(...)
    const play = stmt as ManimPlayAnimation;
    const runTimeMs = (play.runTime ?? DEFAULT_RUN_TIME) * 1000;

    for (const animCall of play.animations) {
      const varId = elementIdByVar.get(animCall.targetId);
      if (!varId) {
        // Target might be a group or unmapped variable
        warnings.push({
          message: `Animation target "${animCall.targetId}" not found in mapped elements.`,
        });
        continue;
      }

      switch (animCall.animationType) {
        case "Create":
        case "Write":
        case "GrowFromCenter":
        case "FadeIn": {
          const anim = buildFadeInAnimation(varId, cursorMs, runTimeMs);
          animations.push(anim);
          if (!firstAppearMs.has(varId)) firstAppearMs.set(varId, cursorMs);
          break;
        }
        case "FadeOut": {
          const anim = buildFadeOutAnimation(varId, cursorMs, runTimeMs);
          animations.push(anim);
          break;
        }
        case "Indicate": {
          const anim = buildBreatheAnimation(varId, runTimeMs);
          animations.push(anim);
          break;
        }
        case "Transform":
        case "ReplacementTransform": {
          // Treat as FadeIn on the target (first arg) for simplicity
          const anim = buildFadeInAnimation(varId, cursorMs, runTimeMs);
          animations.push(anim);
          warnings.push({
            message: `${animCall.animationType} on "${animCall.targetId}" approximated as FadeIn.`,
          });
          break;
        }
        default:
          warnings.push({
            message: `Animation type "${animCall.animationType}" on "${animCall.targetId}" is not supported yet.`,
          });
      }
    }

    cursorMs += runTimeMs + DEFAULT_POST_PLAY_PAUSE;
  }

  // Refine element timeframes: start = first appearance
  elements.forEach((el) => {
    const startMs = firstAppearMs.get(el.id) ?? 0;
    el.timeFrame = { start: startMs, end: Math.max(totalDurationMs, startMs + 1000) };
  });

  return {
    elements,
    animations,
    warnings,
    durationMs: Math.max(totalDurationMs, 1000),
  };
}

function computeTotalDuration(scene: ManimScene): number {
  let total = 0;
  for (const stmt of scene.statements) {
    if (stmt.kind === "wait") {
      total += stmt.duration * 1000;
    } else {
      const play = stmt as ManimPlayAnimation;
      const maxChildRunTime = Math.max(
        ...(play.animations.map((a) => (a.runTime ?? DEFAULT_RUN_TIME) * 1000)),
        DEFAULT_RUN_TIME * 1000
      );
      total += (play.runTime ?? 0) * 1000 || maxChildRunTime;
      total += DEFAULT_POST_PLAY_PAUSE;
    }
  }
  return total || 5000; // minimum 5 seconds
}
