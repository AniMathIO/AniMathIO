/**
 * AST types produced by the Manim Python scene parser.
 * These are intermediate representations before translation to AniMathIO elements.
 */

export type ManimColor = string; // e.g. "WHITE", "BLUE", "#FF0000"

export type ManimVector2D = [number, number];
export type ManimVector3D = [number, number, number];

// ---------- Mobject descriptors ----------

export interface ManimText {
  kind: "Text";
  id: string;
  text: string;
  color?: ManimColor;
  fontSize?: number;
  fontWeight?: number;
}

export interface ManimTex {
  kind: "Tex" | "MathTex";
  id: string;
  tex: string;
  color?: ManimColor;
}

export interface ManimRectangle {
  kind: "Rectangle";
  id: string;
  width?: number;
  height?: number;
  color?: ManimColor;
  fillColor?: ManimColor;
}

export interface ManimCircle {
  kind: "Circle";
  id: string;
  radius?: number;
  color?: ManimColor;
  fillColor?: ManimColor;
}

export interface ManimArrow {
  kind: "Arrow";
  id: string;
  start?: ManimVector3D;
  end?: ManimVector3D;
  color?: ManimColor;
}

export interface ManimImageMobject {
  kind: "ImageMobject";
  id: string;
  filename: string;
}

export interface ManimGroup {
  kind: "Group" | "VGroup";
  id: string;
  children: string[]; // variable names of child mobjects
}

export type ManimMobject =
  | ManimText
  | ManimTex
  | ManimRectangle
  | ManimCircle
  | ManimArrow
  | ManimImageMobject
  | ManimGroup;

// ---------- Animation descriptors ----------

export interface ManimPlayAnimation {
  kind: "play";
  animations: ManimAnimationCall[];
  /** run_time override in seconds */
  runTime?: number;
}

export interface ManimWait {
  kind: "wait";
  duration: number; // seconds
}

export type ManimSceneStatement = ManimPlayAnimation | ManimWait;

export interface ManimAnimationCall {
  animationType:
    | "Create"
    | "Write"
    | "FadeIn"
    | "FadeOut"
    | "GrowFromCenter"
    | "Indicate"
    | "Transform"
    | "ReplacementTransform"
    | "ApplyMethod"
    | "Succession"
    | "AnimationGroup"
    | "SlideIn"
    | "SlideOut"
    | "Other";
  targetId: string;   // variable name of the target mobject
  extraArgs?: string[]; // raw string extra args for complex cases
  runTime?: number;
}

// ---------- Scene descriptor ----------

export interface ManimScene {
  className: string;
  mobjects: Record<string, ManimMobject>;
  statements: ManimSceneStatement[];
}

// ---------- Translation result ----------

export interface ManimTranslationWarning {
  message: string;
  line?: number;
}

export interface ManimTranslationResult {
  elements: import("@/types").EditorElement[];
  animations: import("@/types").Animation[];
  warnings: ManimTranslationWarning[];
  /** Total timeline duration in milliseconds */
  durationMs: number;
}
