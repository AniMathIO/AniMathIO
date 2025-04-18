import { fabric } from "fabric";
import { KatexOptions } from "katex";
import { vec } from "mafs";

export type EditorElementBase<T extends string, P> = {
  readonly id: string;
  fabricObject?: fabric.Object;
  name: string;
  readonly type: T;
  placement: Placement;
  timeFrame: TimeFrame;
  properties: P;
};
export type VideoEditorElement = EditorElementBase<
  "video",
  {
    src: string;
    elementId: string;
    imageObject?: fabric.Image;
    effect: Effect;
    muted: boolean;
  }
>;
export type ImageEditorElement = EditorElementBase<
  "image",
  {
    src: string;
    elementId: string;
    imageObject?: fabric.Object;
    effect: Effect;
  }
>;

export type MafsEditorElement = EditorElementBase<
  "mafs",
  {
    src: string;
    elementId: string;
    imageObject?: fabric.Object;
    effect: Effect;
  }
>;

export type AudioEditorElement = EditorElementBase<
  "audio",
  {
    src: string;
    elementId: string;
    volume: number;
    muted: boolean;
    masterVolume?: number;
  }
>;
export type TextEditorElement = EditorElementBase<
  "text",
  {
    text: string;
    fontSize: number;
    fontWeight: number;
    splittedTexts: fabric.Text[];
  }
>;

export type EditorElement =
  | VideoEditorElement
  | ImageEditorElement
  | AudioEditorElement
  | TextEditorElement
  | MafsEditorElement;

export type Placement = {
  x: number;
  y: number;
  width: number;
  height: number;
  rotation: number;
  scaleX: number;
  scaleY: number;
};

export type TimeFrame = {
  start: number;
  end: number;
};

export type EffectBase<T extends string> = {
  type: T;
};

export type BlackAndWhiteEffect =
  | EffectBase<"none">
  | EffectBase<"blackAndWhite">
  | EffectBase<"sepia">
  | EffectBase<"invert">
  | EffectBase<"saturate">;
export type Effect = BlackAndWhiteEffect;
export type EffectType = Effect["type"];

export type AnimationBase<T, P = {}> = {
  id: string;
  targetId: string;
  duration: number;
  type: T;
  properties: P;
};

export type FadeInAnimation = AnimationBase<"fadeIn">;
export type FadeOutAnimation = AnimationBase<"fadeOut">;

export type BreatheAnimation = AnimationBase<"breathe">;

export type SlideDirection = "left" | "right" | "top" | "bottom";
export type SlideTextType = "none" | "character";
export type SlideInAnimation = AnimationBase<
  "slideIn",
  {
    direction: SlideDirection;
    useClipPath: boolean;
    textType: "none" | "character";
  }
>;

export type SlideOutAnimation = AnimationBase<
  "slideOut",
  {
    direction: SlideDirection;
    useClipPath: boolean;
    textType: SlideTextType;
  }
>;

export type Animation =
  | FadeInAnimation
  | FadeOutAnimation
  | SlideInAnimation
  | SlideOutAnimation
  | BreatheAnimation;

export type MenuOption =
  | "Videos"
  | "Audios"
  | "Audio Mixer"
  | "Text"
  | "Images"
  | "Export"
  | "Animations"
  | "Effects"
  | "Background Fill"
  | "Mathematical Objects";

export type MafsResourceType = {
  index: number;
  name: string;
  children: React.ReactNode;
  coordinates?: boolean;
  coordinateType?: "cartesian" | "polar" | "none";
};

export type MafsModalProps = {
  isOpen: boolean;
  onClose: () => void;
  mafsElement: MafsResourceType;
  className?: string;
};

export type LatexProps = {
  tex: string;
  at: vec.Vector2;
  color?: string;
  katexOptions?: KatexOptions;
};
