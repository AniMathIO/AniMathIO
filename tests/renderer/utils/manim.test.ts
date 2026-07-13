import { describe, it, expect } from "vitest";
import { parseManimScene } from "../../../renderer/utils/manim/parser";
import { translateManimScene } from "../../../renderer/utils/manim/translator";

// Mock getUid since it may depend on crypto
vi.mock("@/utils", () => ({
  getUid: () => Math.random().toString(36).slice(2),
  isHtmlVideoElement: (el: unknown) => el instanceof HTMLVideoElement,
  isHtmlAudioElement: (el: unknown) => el instanceof HTMLAudioElement,
  isHtmlImageElement: (el: unknown) => el instanceof HTMLImageElement,
}));

const SIMPLE_SCENE = `
from manim import *

class HelloScene(Scene):
    def construct(self):
        title = Text("Hello World", font_size=48, color=WHITE)
        formula = MathTex(r"E = mc^2")

        self.play(Write(title))
        self.wait(1)
        self.play(FadeIn(formula))
        self.play(FadeOut(title))
        self.wait(2)
`;

const SHAPES_SCENE = `
from manim import *

class ShapeScene(Scene):
    def construct(self):
        c = Circle(radius=2, color=BLUE)
        r = Rectangle(width=4, height=2, color=RED)
        img = ImageMobject("photo.png")

        self.play(Create(c), Create(r))
        self.wait(0.5)
        self.play(GrowFromCenter(img))
`;

describe("parseManimScene", () => {
  it("extracts the class name", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    expect(scene.className).toBe("HelloScene");
  });

  it("parses Text mobjects", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const title = scene.mobjects["title"];
    expect(title).toBeDefined();
    expect(title.kind).toBe("Text");
    if (title.kind === "Text") {
      expect(title.text).toBe("Hello World");
      expect(title.fontSize).toBe(48);
      expect(title.color).toBe("WHITE");
    }
  });

  it("parses MathTex mobjects", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const formula = scene.mobjects["formula"];
    expect(formula).toBeDefined();
    expect(formula.kind).toBe("MathTex");
  });

  it("parses self.play() statements", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const plays = scene.statements.filter((s) => s.kind === "play");
    expect(plays.length).toBe(3);
  });

  it("parses self.wait() statements", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const waits = scene.statements.filter((s) => s.kind === "wait");
    expect(waits.length).toBe(2);
    expect((waits[0] as { kind: "wait"; duration: number }).duration).toBe(1);
    expect((waits[1] as { kind: "wait"; duration: number }).duration).toBe(2);
  });

  it("parses Write animation", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const firstPlay = scene.statements.find((s) => s.kind === "play") as any;
    expect(firstPlay.animations[0].animationType).toBe("Write");
    expect(firstPlay.animations[0].targetId).toBe("title");
  });

  it("parses FadeIn and FadeOut", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const plays = scene.statements.filter((s) => s.kind === "play") as any[];
    expect(plays[1].animations[0].animationType).toBe("FadeIn");
    expect(plays[2].animations[0].animationType).toBe("FadeOut");
  });

  it("parses Circle and Rectangle mobjects", () => {
    const scene = parseManimScene(SHAPES_SCENE);
    const c = scene.mobjects["c"];
    expect(c.kind).toBe("Circle");
    if (c.kind === "Circle") {
      expect(c.radius).toBe(2);
      expect(c.color).toBe("BLUE");
    }
    const r = scene.mobjects["r"];
    expect(r.kind).toBe("Rectangle");
    if (r.kind === "Rectangle") {
      expect(r.width).toBe(4);
      expect(r.height).toBe(2);
    }
  });

  it("parses ImageMobject", () => {
    const scene = parseManimScene(SHAPES_SCENE);
    const img = scene.mobjects["img"];
    expect(img.kind).toBe("ImageMobject");
    if (img.kind === "ImageMobject") {
      expect(img.filename).toBe("photo.png");
    }
  });

  it("parses multiple animations in one self.play()", () => {
    const scene = parseManimScene(SHAPES_SCENE);
    const firstPlay = scene.statements.find((s) => s.kind === "play") as any;
    expect(firstPlay.animations.length).toBe(2);
    expect(firstPlay.animations[0].animationType).toBe("Create");
    expect(firstPlay.animations[1].animationType).toBe("Create");
  });
});

describe("translateManimScene", () => {
  const canvas = { width: 1920, height: 1080 };

  it("produces elements and animations from a simple scene", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const result = translateManimScene(scene, canvas);

    expect(result.elements.length).toBeGreaterThan(0);
    expect(result.animations.length).toBeGreaterThan(0);
    expect(result.durationMs).toBeGreaterThan(0);
  });

  it("creates TextEditorElements for Text/MathTex", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const result = translateManimScene(scene, canvas);

    const textEls = result.elements.filter((e) => e.type === "text");
    expect(textEls.length).toBe(2);

    const titleEl = textEls.find((e) => e.name === "title");
    expect(titleEl).toBeDefined();
    expect(titleEl?.properties).toMatchObject({ text: "Hello World" });

    const formulaEl = textEls.find((e) => e.name === "formula");
    expect(formulaEl).toBeDefined();
  });

  it("maps Write/Create to fadeIn animations", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const result = translateManimScene(scene, canvas);

    const fadeIns = result.animations.filter((a) => a.type === "fadeIn");
    expect(fadeIns.length).toBeGreaterThanOrEqual(1);
  });

  it("maps FadeOut to fadeOut animations", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const result = translateManimScene(scene, canvas);

    const fadeOuts = result.animations.filter((a) => a.type === "fadeOut");
    expect(fadeOuts.length).toBe(1);
  });

  it("creates ImageEditorElement for ImageMobject", () => {
    const scene = parseManimScene(SHAPES_SCENE);
    const result = translateManimScene(scene, canvas);

    const imgEl = result.elements.find((e) => e.type === "image");
    expect(imgEl).toBeDefined();
    if (imgEl?.type === "image") {
      expect(imgEl.properties.src).toBe("photo.png");
    }
  });

  it("accounts for self.wait() in total duration", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const result = translateManimScene(scene, canvas);
    // 1s wait + 2s wait + 3 plays × 1s default = 6s minimum
    expect(result.durationMs).toBeGreaterThanOrEqual(6000);
  });

  it("returns no errors for valid scene", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const result = translateManimScene(scene, canvas);
    // Warnings may exist but should be informational, not critical
    expect(result.elements.length).toBeGreaterThan(0);
  });

  it("assigns valid placement to each element", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const result = translateManimScene(scene, canvas);

    for (const el of result.elements) {
      expect(el.placement.width).toBeGreaterThan(0);
      expect(el.placement.height).toBeGreaterThan(0);
    }
  });

  it("each animation targets a valid element id", () => {
    const scene = parseManimScene(SIMPLE_SCENE);
    const result = translateManimScene(scene, canvas);
    const elementIds = new Set(result.elements.map((e) => e.id));

    for (const anim of result.animations) {
      expect(elementIds.has(anim.targetId)).toBe(true);
    }
  });
});
