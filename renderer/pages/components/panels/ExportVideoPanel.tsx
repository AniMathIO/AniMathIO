"use client";
import React from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import Modal from "react-modal";


const ExportVideoPanel = observer(() => {
  const state = React.useContext(StateContext);
  const [resolution, setResolution] = React.useState('1920x1080');
  const [isRenderingModalOpen, setIsRenderingModalOpen] = React.useState(false);

  const resolutionOptions = [
    { value: '1920x1080', label: '1920x1080 (16:9 - Full HD)' },
    { value: '1280x720', label: '1280x720 (16:9 - HD)' },
    { value: '1080x1080', label: '1080x1080 (1:1 - Square)' },
    { value: '1080x1920', label: '1080x1920 (9:16 - Vertical Full HD)' },
    { value: '720x1280', label: '720x1280 (9:16 - Vertical HD)' },
    { value: '854x480', label: '854x480 (16:9 - SD)' },
    { value: '800x600', label: '800x600 (4:3 - SD)' },
    { value: '640x360', label: '640x360 (16:9 - SD)' },
  ];
  const handleResolutionChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const [width, height] = e.target.value.split('x').map(Number);
    state.setCanvasSize(width, height);
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
            className="rounded text-center text-black border-slate-200 placeholder-slate-400 contrast-more:border-slate-400 contrast-more:placeholder-slate-500 max-w-[50px] mr-2"
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
            value={`${state.canvas_width}x${state.canvas_height}`}
            onChange={handleResolutionChange}
            className="rounded-md text-base text-black w-24 border-slate-200 placeholder-slate-400 contrast-more:border-slate-400 contrast-more:placeholder-slate-500"
          >
            {resolutionOptions.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </div>
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
            value="webm"
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
        className="bg-gray-500 hover:bg-gray-900 text-white font-bold py-2 px-2 rounded-lg m-4"
        onClick={() => {
          state.handleSeek(0);
          state.setSelectedElement(null);
          setTimeout(() => {
            state.setPlaying(true);
            state.saveCanvasToVideoWithAudio();
          }, 1000);
          setIsRenderingModalOpen(true);
          setTimeout(() => {
            setIsRenderingModalOpen(false);
            // If the rendering option is mp4 we need to wait additional 15 seconds to do the conversion
          }, state.maxTime + 1000 + (state.selectedVideoFormat === "mp4" ? 15000 : 0));
        }}
      >
        Export Video ({state.maxTime / 1000} seconds)
      </button >
      <Modal
        isOpen={isRenderingModalOpen}
        onRequestClose={() => setIsRenderingModalOpen(false)}
        contentLabel="Video Rendering Modal"
        className="fixed inset-0 flex items-center justify-center z-50 bg-black bg-opacity-50"
        overlayClassName="fixed inset-0 z-40 bg-black bg-opacity-60"
        shouldCloseOnOverlayClick={false}
      >
        <div className="bg-white p-6 rounded shadow-lg max-w-md mx-auto">
          <div className="flex items-center mb-4">
            <div className="mr-4">
              <svg
                className="animate-spin h-8 w-8 text-blue-500"
                xmlns="http://www.w3.org/2000/svg"
                fill="none"
                viewBox="0 0 24 24"
              >
                <circle
                  className="opacity-25"
                  cx="12"
                  cy="12"
                  r="10"
                  stroke="currentColor"
                  strokeWidth="4"
                ></circle>
                <path
                  className="opacity-75"
                  fill="currentColor"
                  d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                ></path>
              </svg>
            </div>
            <div>
              <h2 className="text-xl font-bold">Video Rendering</h2>
              <p className="text-gray-700">Please wait while the video is being rendered...</p>
              <br />
              <p className="text-gray-800"><b>Note:</b> rendering in MP4 results in higher quality, but takes longer.</p>
            </div>
          </div>
        </div>
      </Modal>
    </>
  );
});

export default ExportVideoPanel