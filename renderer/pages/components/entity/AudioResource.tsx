"use client";
import React from "react";
import { StateContext } from "../../states";
import { formatTimeToMinSec } from "../../utils";
import { observer } from "mobx-react";
import { PlusCircleIcon, PlayIcon, PauseIcon } from "@heroicons/react/24/solid";
import WavesurferPlayer from "@wavesurfer/react";

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

    const [wavesurfer, setWavesurfer] = React.useState<any>(null)
    const [isPlaying, setIsPlaying] = React.useState(false)

    const onReady = (ws: any) => {
      setWavesurfer(ws)
      setIsPlaying(false)
    }

    const onPlayPause = () => {
      wavesurfer && wavesurfer?.playPause();
    }

    return (
      <div className="rounded-lg overflow-hidden items-center bg-slate-800 m-[15px] flex flex-col relative min-h-[130px]">
        {/* align vertically center */}
        <div className="w-full flex justify-start px-1">
          <WavesurferPlayer
            url={audio}
            height={130}
            width={200}
            autoCenter={true}
            autoScroll={true}
            waveColor="#00a0f5"
            progressColor="#027bbca9"
            onReady={onReady}
            fillParent={true}
            onPlay={() => setIsPlaying(true)}
            onPause={() => setIsPlaying(false)}
          />
        </div>
        <div className=" text-white py-1 absolute text-base top-2 right-2">
          {formatedAudioLength}
        </div>
        <button onClick={onPlayPause} className="hover:bg-[#00a0f5] rounded z-10 text-white font-bold py-1 absolute text-lg top-10 right-2">
          {isPlaying ? <PauseIcon className="h-8 w-8" /> : <PlayIcon className="h-8 w-8" />}
        </button>

        <button
          aria-label="Add audio"
          className="hover:bg-[#00a0f5] rounded z-10 text-white font-bold py-1 absolute text-lg bottom-2 right-2"
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
