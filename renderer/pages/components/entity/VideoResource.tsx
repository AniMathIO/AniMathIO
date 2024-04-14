"use client";
import React from "react";
import { StateContext } from "../../states";
import { formatTimeToMinSec } from "../../utils";
import { observer } from "mobx-react";
import { PlusCircleIcon } from "@heroicons/react/24/solid";

type VideoResourceProps = {
  video: string;
  index: number;
};
export const VideoResource = observer(
  ({ video, index }: VideoResourceProps) => {
    const state = React.useContext(StateContext);
    const ref = React.useRef<HTMLVideoElement>(null);
    const [formatedVideoLength, setFormatedVideoLength] =
      React.useState("00:00");

    return (
      <div className="rounded-lg overflow-hidden items-center bg-slate-800 m-[15px] flex flex-col relative">
        <div className="bg-[rgba(0,0,0,.25)] text-white py-1 absolute text-base top-2 right-2">
          {formatedVideoLength}
        </div>
        <button
          aria-label="Add video"
          className="hover:bg-[#00a0f5] bg-[rgba(0,0,0,.25)] rounded z-10 text-white font-bold py-1 absolute text-lg bottom-2 right-2"
          onClick={() => state.addVideo(index)}
        >
          <PlusCircleIcon className="h-8 w-8"></PlusCircleIcon>
        </button>
        <video
          onLoadedData={() => {
            const videoLength = ref.current?.duration ?? 0;
            setFormatedVideoLength(formatTimeToMinSec(videoLength));
          }}
          ref={ref}
          className="max-h-[100px] max-w-[150px]"
          src={video}
          height={200}
          width={200}
          id={`video-${index}`}
        ></video>
      </div>
    );
  }
);
