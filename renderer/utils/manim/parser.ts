/**
 * Manim Python scene parser.
 *
 * Parses a subset of Manim Community Python scripts using regex-based line-by-line
 * analysis (no full Python AST). Handles:
 *  - Class definition inheriting from Scene
 *  - construct() method body
 *  - Variable assignments of common Mobjects (Text, Tex, MathTex, Circle, Rectangle,
 *    Arrow, ImageMobject, VGroup/Group)
 *  - self.play() calls with Create/Write/FadeIn/FadeOut/GrowFromCenter animations
 *  - self.wait() calls
 *  - run_time keyword argument
 */

import type {
  ManimScene,
  ManimMobject,
  ManimSceneStatement,
  ManimPlayAnimation,
  ManimAnimationCall,
  ManimColor,
  ManimVector3D,
} from "./types";

// ---------- helpers ----------

function stripComments(line: string): string {
  const idx = line.indexOf("#");
  return idx >= 0 ? line.slice(0, idx) : line;
}

function trimQuotes(s: string): string {
  return s.replace(/^['"]|['"]$/g, "");
}

function parseColor(raw: string): ManimColor | undefined {
  raw = raw.trim();
  if (!raw) return undefined;
  // Named Manim colors or hex
  if (/^[A-Z_]+$/.test(raw)) return raw;
  if (/^['"]#[0-9a-fA-F]{3,8}['"]$/.test(raw)) return trimQuotes(raw);
  return raw;
}

function parseNumber(raw: string): number | undefined {
  const n = parseFloat(raw.trim());
  return isNaN(n) ? undefined : n;
}

/** Extract keyword argument value from a raw argument list string */
function extractKwarg(args: string, key: string): string | undefined {
  const re = new RegExp(`${key}\\s*=\\s*([^,)]+)`);
  const m = args.match(re);
  return m ? m[1].trim() : undefined;
}

/** Parse a simple Python list/tuple of numbers, e.g. [1, 2, 0] or (1.5, -2, 0) */
function parseVector3D(raw: string): ManimVector3D | undefined {
  const m = raw.match(/[\[(]([\d.,\s\-]+)[\])]/);
  if (!m) return undefined;
  const parts = m[1].split(",").map((s) => parseFloat(s.trim()));
  if (parts.length < 2 || parts.some(isNaN)) return undefined;
  return [parts[0], parts[1], parts[2] ?? 0];
}

/** Extract the content inside the outermost parentheses */
function extractParens(s: string, open = 0): string {
  let depth = 0;
  let start = -1;
  for (let i = open; i < s.length; i++) {
    if (s[i] === "(") {
      if (depth === 0) start = i + 1;
      depth++;
    } else if (s[i] === ")") {
      depth--;
      if (depth === 0) return s.slice(start, i);
    }
  }
  return "";
}

// ---------- Mobject parsers ----------

function parseMobjectCall(varName: string, rhs: string): ManimMobject | null {
  rhs = rhs.trim();

  const mobjectFactories: Array<[string, (args: string) => ManimMobject]> = [
    [
      "Text",
      (args) => {
        // Text("Hello", color=WHITE, font_size=48)
        const textMatch = args.match(/^['"](.+?)['"]/);
        return {
          kind: "Text",
          id: varName,
          text: textMatch ? textMatch[1] : args,
          color: parseColor(extractKwarg(args, "color") ?? ""),
          fontSize: parseNumber(extractKwarg(args, "font_size") ?? ""),
          fontWeight: parseNumber(extractKwarg(args, "font_weight") ?? ""),
        };
      },
    ],
    [
      "MathTex",
      (args) => ({
        kind: "MathTex",
        id: varName,
        tex: trimQuotes(args.split(",")[0].trim()),
        color: parseColor(extractKwarg(args, "color") ?? ""),
      }),
    ],
    [
      "Tex",
      (args) => ({
        kind: "Tex",
        id: varName,
        tex: trimQuotes(args.split(",")[0].trim()),
        color: parseColor(extractKwarg(args, "color") ?? ""),
      }),
    ],
    [
      "Circle",
      (args) => ({
        kind: "Circle",
        id: varName,
        radius: parseNumber(extractKwarg(args, "radius") ?? args.split(",")[0]),
        color: parseColor(extractKwarg(args, "color") ?? ""),
        fillColor: parseColor(extractKwarg(args, "fill_color") ?? ""),
      }),
    ],
    [
      "Rectangle",
      (args) => ({
        kind: "Rectangle",
        id: varName,
        width: parseNumber(extractKwarg(args, "width") ?? ""),
        height: parseNumber(extractKwarg(args, "height") ?? ""),
        color: parseColor(extractKwarg(args, "color") ?? ""),
        fillColor: parseColor(extractKwarg(args, "fill_color") ?? ""),
      }),
    ],
    [
      "Arrow",
      (args) => {
        const parts = args.split(",");
        return {
          kind: "Arrow",
          id: varName,
          start: parseVector3D(parts[0] ?? ""),
          end: parseVector3D(parts[1] ?? ""),
          color: parseColor(extractKwarg(args, "color") ?? ""),
        };
      },
    ],
    [
      "ImageMobject",
      (args) => ({
        kind: "ImageMobject",
        id: varName,
        filename: trimQuotes(args.split(",")[0].trim()),
      }),
    ],
  ];

  for (const [factory, builder] of mobjectFactories) {
    if (rhs.startsWith(factory + "(")) {
      try {
        const args = extractParens(rhs, factory.length);
        return builder(args);
      } catch {
        return null;
      }
    }
  }

  // VGroup / Group
  for (const g of ["VGroup", "Group"]) {
    if (rhs.startsWith(g + "(")) {
      const args = extractParens(rhs, g.length);
      const children = args
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      return { kind: g as "VGroup" | "Group", id: varName, children };
    }
  }

  return null;
}

// ---------- Animation parsers ----------

const ANIMATION_TYPES = [
  "Create",
  "Write",
  "FadeIn",
  "FadeOut",
  "GrowFromCenter",
  "Indicate",
  "Transform",
  "ReplacementTransform",
  "ApplyMethod",
  "Succession",
  "AnimationGroup",
] as const;

function parseAnimationCall(raw: string): ManimAnimationCall | null {
  raw = raw.trim();

  for (const animType of ANIMATION_TYPES) {
    if (raw.startsWith(animType + "(")) {
      const args = extractParens(raw, animType.length);
      const firstArg = args.split(",")[0].trim();
      const runTime = parseNumber(extractKwarg(args, "run_time") ?? "");
      return {
        animationType: animType,
        targetId: firstArg,
        runTime,
      };
    }
  }

  return { animationType: "Other", targetId: raw };
}

/** Split top-level comma-separated items (not inside parens) */
function splitTopLevel(s: string): string[] {
  const result: string[] = [];
  let depth = 0;
  let current = "";
  for (const ch of s) {
    if (ch === "(" || ch === "[" || ch === "{") depth++;
    else if (ch === ")" || ch === "]" || ch === "}") depth--;
    else if (ch === "," && depth === 0) {
      result.push(current.trim());
      current = "";
      continue;
    }
    current += ch;
  }
  if (current.trim()) result.push(current.trim());
  return result;
}

function parseSelfPlay(line: string): ManimPlayAnimation | null {
  const m = line.match(/self\.play\s*\(/);
  if (!m) return null;

  const args = extractParens(line, line.indexOf("("));
  const parts = splitTopLevel(args);

  const runTimeStr = extractKwarg(args, "run_time");
  const runTime = runTimeStr ? parseNumber(runTimeStr) : undefined;

  const animations: ManimAnimationCall[] = [];
  for (const part of parts) {
    if (part.startsWith("run_time")) continue;
    const anim = parseAnimationCall(part);
    if (anim) animations.push(anim);
  }

  return { kind: "play", animations, runTime };
}

function parseSelfWait(line: string): { duration: number } | null {
  const m = line.match(/self\.wait\s*\(\s*([\d.]*)\s*\)/);
  if (!m) return null;
  return { duration: parseNumber(m[1]) ?? 1 };
}

// ---------- Main parser ----------

export function parseManimScene(source: string): ManimScene {
  const lines = source.split("\n");
  let className = "UnknownScene";
  let inConstruct = false;
  let constructIndent = 0;
  const mobjects: Record<string, ManimMobject> = {};
  const statements: ManimSceneStatement[] = [];

  // Find Scene class
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const line = stripComments(raw);

    // class definition
    const classMatch = line.match(/^class\s+(\w+)\s*\(.*Scene.*\)\s*:/);
    if (classMatch) {
      className = classMatch[1];
      continue;
    }

    // construct method start
    const constructMatch = line.match(/^(\s+)def\s+construct\s*\(self\)\s*:/);
    if (constructMatch) {
      inConstruct = true;
      constructIndent = constructMatch[1].length + 4; // expect body at least this indented
      continue;
    }

    if (!inConstruct) continue;

    // Detect end of construct (new method at same or lower indent as "def construct")
    const newMethodMatch = line.match(/^(\s+)def\s+\w+/);
    if (newMethodMatch && newMethodMatch[1].length <= constructIndent - 4) {
      inConstruct = false;
      continue;
    }

    const stripped = line.trim();
    if (!stripped) continue;

    // Join multi-line self.play calls
    let fullLine = stripped;
    if (/self\.play\s*\(/.test(fullLine) && !fullLine.includes(")")) {
      let j = i + 1;
      while (j < lines.length && !lines[j].includes(")")) {
        fullLine += " " + lines[j].trim();
        j++;
      }
      if (j < lines.length) fullLine += " " + lines[j].trim();
      i = j;
    }

    // self.play(...)
    const play = parseSelfPlay(fullLine);
    if (play) {
      statements.push(play);
      continue;
    }

    // self.wait(...)
    const wait = parseSelfWait(fullLine);
    if (wait) {
      statements.push({ kind: "wait", duration: wait.duration });
      continue;
    }

    // Variable assignment  varName = MobjectFactory(...)
    const assignMatch = stripped.match(/^(\w+)\s*=\s*(.+)$/);
    if (assignMatch) {
      const [, varName, rhs] = assignMatch;
      const mob = parseMobjectCall(varName, rhs.trim());
      if (mob) {
        mobjects[varName] = mob;
      }
    }
  }

  return { className, mobjects, statements };
}
