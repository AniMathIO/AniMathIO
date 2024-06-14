"use client";

import { fabric } from "fabric";
import React, { useEffect, useState } from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import Resources from "./components/Resources";
import ElementsPanel from "./components/panels/ElementsPanel";
import Menu from "./Menu";
import Timeline from "./components/Timeline";
import { State } from '../states/state';
import Head from "next/head";
import AniMathIO from "../public/images/AniMathIO.png";

export const EditorWithStore = () => {
  const [state] = useState(new State());
  return (
    <StateContext.Provider value={state}>
      <Editor></Editor>
    </StateContext.Provider>
  );
}

const Editor = observer(() => {
  const state = React.useContext(StateContext);
  const [scaleFactor, setScaleFactor] = React.useState(25);

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

  useEffect(() => {
    const canvas = new fabric.Canvas("canvas", {
      height: state.canvas_height,
      width: state.canvas_width,
      backgroundColor: "#ededed",
    });
    fabric.Object.prototype.transparentCorners = false;
    fabric.Object.prototype.cornerColor = "#00a0f5";
    fabric.Object.prototype.cornerStyle = "circle";
    fabric.Object.prototype.cornerStrokeColor = "#0063d8";
    fabric.Object.prototype.cornerSize = 10;

    canvas.on("mouse:down", function (e) {
      if (!e.target) {
        state.setSelectedElement(null);
      }
    });

    state.setCanvas(canvas, state.canvas_width, state.canvas_height);
    fabric.util.requestAnimFrame(function render() {
      canvas.renderAll();
      fabric.util.requestAnimFrame(render);
    });

    // Set the default scale factor based on the initial canvas dimensions
    const defaultScaleFactor = canvasScaleMap[`${state.canvas_width}x${state.canvas_height}`]?.default || 37;
    setScaleFactor(defaultScaleFactor);
  }, []);

  useEffect(() => {
    // Update the scale factor when the canvas dimensions change
    const defaultScaleFactor = canvasScaleMap[`${state.canvas_width}x${state.canvas_height}`]?.default || 37;
    setScaleFactor(defaultScaleFactor);
  }, [state.canvas_width, state.canvas_height]);

  const handleScaleChange = (event: any) => {
    const newScaleFactor = parseInt(event.target.value);
    const key = `${state.canvas_width}x${state.canvas_height}`;
    const maxScaleFactor = canvasScaleMap[key]?.max || 100;
    setScaleFactor(Math.min(newScaleFactor, maxScaleFactor));
  };

  return (
    <React.Fragment>
      <Head>
        <title>AniMathIO - Editor</title>
        <link rel="icon" href={AniMathIO.src} />
      </Head>

      <div className="bg-slate-200 dark:bg-gray-800 grid grid-rows-[500px_1fr_20px] grid-cols-[90px_300px_250px_1fr] h-[100svh]">
        <div className="tile row-span-2 flex flex-col">
          <Menu />
        </div>
        <div className="row-span-2 flex flex-col overflow-scroll">
          <Resources />
        </div>
        <div className="col-start-3 row-start-1">
          <ElementsPanel />
        </div>

        <div id="grid-canvas-container" className="col-start-4 bg-gray-200 dark:bg-gray-700 dark:text-white grid w-[900px] h-[500px] place-self-center place-content-center">
          <div
            style={{
              transformOrigin: "",
              transform: `scale(${scaleFactor / 100})`,
            }}
            className={`flex w-fit h-fit`}
          >
            <canvas id="canvas" className=""></canvas>
          </div>
          <div className="mt-4 absolute justify-self-start self-end p-2">
            <p>Canva Scale:</p>
            <input
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

    </React.Fragment >
  );
});

export default Editor;