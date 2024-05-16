"use client";
import React from "react";
import { StateContext } from "../../../states";
import { observer } from "mobx-react";



const ExportVideoPanel = observer(() => {
  const state = React.useContext(StateContext);
  const [resolution, setResolution] = React.useState('1920x1080');

  React.useEffect(() => {
    setResolution('800x500');
  }, []);

  const handleResolutionChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    setResolution(e.target.value);
  };

  return (
    <>
      <div className="text-lg px-[16px] pt-[16px] pb-[15px] font-semibold">
        Export
      </div>
      {/* Set max time from number input */}
      <div className="px-[16px]">
        <div className="flex flex-row items-center my-2">
          <div className="text-base font-semibold mr-2">Video Length:</div>
          <label htmlFor="video-length" className="sr-only">Video Length</label>
          <input
            id="video-length"
            type="number"
            className="rounded text-center border-slate-200 placeholder-slate-400 contrast-more:border-slate-400 contrast-more:placeholder-slate-500 max-w-[50px] mr-2"
            value={state.maxTime / 1000}
            onChange={(e) => {
              const value = e.target.value;
              state.setMaxTime(Number(value) * 1000);
            }}
            placeholder="Length in seconds"
          />
          <div>secs</div>
        </div>
        <div className="flex flex-row items-center my-2">
          <div className="text-base font-semibold mr-2">Canvas Resolution:</div>
          <label htmlFor="canvas-resolution" className="sr-only">Canvas Resolution</label>
          <select
            id="canvas-resolution"
            name="canvas-resolution"
            value={resolution}
            onChange={handleResolutionChange}
            disabled={true}
            className="rounded-md text-base border-slate-200 placeholder-slate-400 contrast-more:border-slate-400 contrast-more:placeholder-slate-500"
          >
            <option value="1920x1080">1920x1080 (Full HD)</option>
            <option value="1280x720">1280x720 (HD)</option>
            <option value="854x480">854x480 (SD)</option>
            <option value="800x500">800x500 (SD)</option>
            <option value="640x360">640x360 (SD)</option>
          </select>

        </div >
      </div >
      {/*  Format selection with radio button */}
      <div className="px-[16px]" >
        <div className="text-base font-semibold mr-2">Video Format:</div>
        <div className="flex flex-row items-center my-2">
          <label htmlFor="video-format" className="sr-only">Video Format</label>
          <input
            id="video-format"
            type="radio"
            className="mr-2"
            name="video-format"
            value="mp4"
            checked={state.selectedVideoFormat === "mp4"}
            onChange={(e) => {
              state.setVideoFormat("mp4");
            }}
            placeholder="Video Format"
          />
          <div className="text-md mr-2">MP4</div>
          <label htmlFor="video-format" ></label>
          <input
            id="video-format"
            type="radio"
            className="mr-2"
            name="video-format"
            value="gif"
            checked={state.selectedVideoFormat === "webm"}
            onChange={(e) => {
              state.setVideoFormat("webm");
            }}
            placeholder="Video Format"
          />
          <div className="text-md mr-2">webm</div>
        </div>
      </div>

      <button
        className="bg-gray-500 hover:bg-gray-700 text-white font-bold py-2 px-2 rounded-lg m-4"
        onClick={() => {
          state.handleSeek(0);
          state.setSelectedElement(null);
          setTimeout(() => {
            state.setPlaying(true);
            state.saveCanvasToVideoWithAudio();
          }, 1000);
        }}
      >
        Export Video ({state.maxTime / 1000} seconds) {state.selectedVideoFormat === "mp4" ? ("experimental") : ""}
      </button>
    </>
  );
});

export default ExportVideoPanel