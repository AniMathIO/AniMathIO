"use client";
import React, { useCallback, useContext, useRef, useState } from "react";
import { observer } from "mobx-react";
import { StateContext } from "@/states";
import { parseManimScene, translateManimScene } from "@/utils/manim";
import type { ManimTranslationWarning } from "@/utils/manim";
import { runInAction } from "mobx";

const EXAMPLE_SCRIPT = `from manim import *

class HelloMath(Scene):
    def construct(self):
        title = Text("Welcome to AniMathIO", font_size=48, color=WHITE)
        formula = MathTex(r"E = mc^2", color=YELLOW)
        subtitle = Text("Mathematical animations made easy", font_size=28, color=GREY)

        self.play(Write(title))
        self.wait(1)
        self.play(FadeIn(formula))
        self.play(Indicate(formula))
        self.wait(0.5)
        self.play(FadeOut(title), FadeIn(subtitle))
        self.wait(1)
`;

const ManimImportPanel = observer(() => {
  const state = useContext(StateContext);
  const [script, setScript] = useState(EXAMPLE_SCRIPT);
  const [warnings, setWarnings] = useState<ManimTranslationWarning[]>([]);
  const [importedCount, setImportedCount] = useState<{ elements: number; animations: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isImporting, setIsImporting] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleFileLoad = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (ev) => {
      setScript(ev.target?.result as string ?? "");
      setImportedCount(null);
      setWarnings([]);
      setError(null);
    };
    reader.readAsText(file);
  }, []);

  const handleImport = useCallback(async () => {
    setError(null);
    setWarnings([]);
    setImportedCount(null);
    setIsImporting(true);

    try {
      const parsed = parseManimScene(script);
      const result = await translateManimScene(parsed, {
        width: state.canvas_width,
        height: state.canvas_height,
      });

      runInAction(() => {
        // Extend maxTime if the scene is longer
        if (result.durationMs > state.maxTime) {
          state.setMaxTime(result.durationMs);
        }

        // Import elements
        for (const el of result.elements) {
          state.addEditorElement(el);
        }

        // Import animations
        for (const anim of result.animations) {
          state.addAnimation(anim);
        }
      });

      setWarnings(result.warnings);
      setImportedCount({ elements: result.elements.length, animations: result.animations.length });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsImporting(false);
    }
  }, [script, state]);

  return (
    <div className="flex flex-col h-full px-4 py-4 gap-3">
      <div className="text-lg font-semibold">Manim Import</div>
      <p className="text-xs text-gray-500 dark:text-gray-400 leading-relaxed">
        Paste or load a Manim Community Python scene script. The parser supports a
        subset of Manim: <code className="bg-gray-200 dark:bg-gray-600 rounded px-1">Text</code>,{" "}
        <code className="bg-gray-200 dark:bg-gray-600 rounded px-1">Tex</code>,{" "}
        <code className="bg-gray-200 dark:bg-gray-600 rounded px-1">MathTex</code>,{" "}
        <code className="bg-gray-200 dark:bg-gray-600 rounded px-1">Circle</code>,{" "}
        <code className="bg-gray-200 dark:bg-gray-600 rounded px-1">Rectangle</code>, and animations like{" "}
        <code className="bg-gray-200 dark:bg-gray-600 rounded px-1">Create</code>,{" "}
        <code className="bg-gray-200 dark:bg-gray-600 rounded px-1">Write</code>,{" "}
        <code className="bg-gray-200 dark:bg-gray-600 rounded px-1">FadeIn</code>,{" "}
        <code className="bg-gray-200 dark:bg-gray-600 rounded px-1">FadeOut</code>.
      </p>

      {/* Toolbar */}
      <div className="flex gap-2 items-center flex-wrap">
        <button
          onClick={() => fileInputRef.current?.click()}
          className="px-3 py-1.5 text-sm rounded bg-gray-200 dark:bg-gray-600 hover:bg-gray-300 dark:hover:bg-gray-500 transition-colors"
        >
          Load .py file
        </button>
        <button
          onClick={() => { setScript(EXAMPLE_SCRIPT); setImportedCount(null); setWarnings([]); setError(null); }}
          className="px-3 py-1.5 text-sm rounded bg-gray-200 dark:bg-gray-600 hover:bg-gray-300 dark:hover:bg-gray-500 transition-colors"
        >
          Load example
        </button>
        <input
          ref={fileInputRef}
          type="file"
          accept=".py"
          className="hidden"
          onChange={handleFileLoad}
        />
      </div>

      {/* Script editor */}
      <textarea
        value={script}
        onChange={(e) => { setScript(e.target.value); setImportedCount(null); }}
        spellCheck={false}
        className="flex-1 min-h-[220px] font-mono text-xs p-3 rounded border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-black dark:text-white resize-none focus:outline-none focus:ring-2 focus:ring-blue-500"
        placeholder="Paste your Manim Python scene script here…"
      />

      {/* Import button */}
      <button
        onClick={handleImport}
        disabled={!script.trim() || isImporting}
        className="w-full py-2 rounded font-semibold text-white bg-blue-600 hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
      >
        {isImporting ? "Importing…" : "Import into Timeline"}
      </button>

      {/* Success */}
      {importedCount !== null && (
        <div className="rounded bg-green-100 dark:bg-green-900 border border-green-400 dark:border-green-600 px-3 py-2 text-sm text-green-800 dark:text-green-200">
          Imported {importedCount.elements} element{importedCount.elements !== 1 ? "s" : ""} and{" "}
          {importedCount.animations} animation{importedCount.animations !== 1 ? "s" : ""}.
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="rounded bg-red-100 dark:bg-red-900 border border-red-400 dark:border-red-600 px-3 py-2 text-sm text-red-800 dark:text-red-200">
          <strong>Parse error:</strong> {error}
        </div>
      )}

      {/* Warnings */}
      {warnings.length > 0 && (
        <div className="rounded bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-400 dark:border-yellow-600 px-3 py-2 text-xs space-y-1">
          <div className="font-semibold text-yellow-800 dark:text-yellow-300 mb-1">
            {warnings.length} warning{warnings.length !== 1 ? "s" : ""}:
          </div>
          {warnings.map((w, i) => (
            <div key={i} className="text-yellow-700 dark:text-yellow-400">• {w.message}</div>
          ))}
        </div>
      )}
    </div>
  );
});

export default ManimImportPanel;
