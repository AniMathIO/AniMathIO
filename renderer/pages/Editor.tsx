"use client";

import { fabric } from "fabric";
import React, { useEffect, useState } from "react";
import { StateContext } from "./states";
import { observer } from "mobx-react";
import { Resources } from "./components/Resources";
import { ElementsPanel } from "./components/panels/ElementsPanel";
import { Menu } from "./Menu";
import { Timeline } from "./components/Timeline";
import { State } from './states/state';
import Head from "next/head";

export const EditorWithStore = () => {
  const [state] = useState(new State());
  return (
    <StateContext.Provider value={state}>
      <Editor></Editor>
    </StateContext.Provider>
  );
}

export const Editor = observer(() => {
  const state = React.useContext(StateContext);

  useEffect(() => {
    const canvas = new fabric.Canvas("canvas", {
      height: 500,
      width: 800,
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

    state.setCanvas(canvas);
    fabric.util.requestAnimFrame(function render() {
      canvas.renderAll();
      fabric.util.requestAnimFrame(render);
    });
  }, []);
  return (
    <React.Fragment>
      <Head>
        <title>AniMathIO - Editor</title>
        <link rel="icon" href="../images/AniMathIO.png" />
      </Head>

      <div className="grid grid-rows-[500px_1fr_20px] grid-cols-[90px_300px_250px_1fr] h-[100svh]">
        <div className="tile row-span-2 flex flex-col">
          <Menu />
        </div>
        <div className="row-span-2 flex flex-col overflow-scroll">
          <Resources />
        </div>
        <div className="col-start-3 row-start-1">
          <ElementsPanel />
        </div>
        <div id="grid-canvas-container" className="col-start-4 bg-slate-100 flex justify-center items-center">
          <canvas id="canvas" className="h-[500px] w-[800px] row" />
        </div>
        <div className="col-start-3 row-start-2 col-span-2 relative px-[10px] py-[4px] overflow-scroll">
          <Timeline />
        </div>
      </div>
    </React.Fragment>
  );
});
