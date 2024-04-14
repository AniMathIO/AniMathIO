"use client";
import React from "react";
import { StateContext } from "../../states";
import { formatTimeToMinSec } from "../../utils";
import { observer } from "mobx-react";
import { PlusCircleIcon } from "@heroicons/react/24/solid";

export type AudioResourceProps = {
  audio: string;
  index: number;
};
export const AudioResource = observer(
  ({ audio, index }: AudioResourceProps) => {
    const state = React.useContext(StateContext);
    const ref = React.useRef<HTMLAudioElement>(null);
    const [formatedAudioLength, setFormatedAudioLength] =
      React.useState("00:00");

    return (
      <div className="rounded-lg overflow-hidden items-center bg-slate-800 m-[15px] flex flex-col relative min-h-[100px]">
        <div className="bg-[rgba(0,0,0,.25)] text-white py-1 absolute text-base top-2 right-2">
          {formatedAudioLength}
        </div>
        <button
          aria-label="Add audio"
          className="hover:bg-[#00a0f5] bg-[rgba(0,0,0,.25)] rounded z-10 text-white font-bold py-1 absolute text-lg bottom-2 right-2"
          onClick={() => state.addAudio(index)}
        >
          <PlusCircleIcon className="h-8 w-8" />
        </button>
        <audio
          onLoadedData={() => {
            const audioLength = ref.current?.duration ?? 0;
            setFormatedAudioLength(formatTimeToMinSec(audioLength));
          }}
          ref={ref}
          className="max-h-[100px] max-w-[150px] min-h-[50px] min-w-[100px]"
          // controls
          src={audio}
          id={`audio-${index}`}
        ></audio>
      </div>
    );
  }
);
