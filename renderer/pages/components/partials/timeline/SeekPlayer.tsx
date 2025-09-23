"use client";

import { StateContext } from "@/states";
import { formatTimeToMinSecMili } from "@/utils";
import { observer } from "mobx-react-lite";
import { useContext } from "react";
import { PlayIcon, PauseIcon } from "@heroicons/react/24/solid";
import { FaStepForward, FaStepBackward } from "react-icons/fa";
import { MdForward10, MdReplay10 } from "react-icons/md";
import ScaleRangeInput from "./ScaleRangeInput";

const MARKINGS = [
    {
        interval: 5000,
        // color: 'black',
        // if tailwind dark mode is set to true then color white otherwise black
        color: 'gray',
        size: 16,
        width: 1
    },
    {
        interval: 1000,
        color: 'gray',
        size: 8,
        width: 1
    }
];

export type SeekPlayerProps = {};


const SeekPlayer = observer((_props: SeekPlayerProps) => {
    const state = useContext(StateContext);
    const Icon = state.playing ? PauseIcon : PlayIcon;
    const formattedTime = formatTimeToMinSecMili(state.currentTimeInMs);
    const formattedMaxTime = formatTimeToMinSecMili(state.maxTime);
    return (
        <div className="seek-player bg-slate-200 dark:bg-gray-800 dark:text-white flex flex-col overflow-hidden">
            <div className="flex flex-row items-center px-2 space-x-2">
                {/* Skip to start */}
                <button
                    className="px-2 py-2"
                    onClick={() => state.skipToStart()}
                    aria-label="Skip to Start"
                >
                    <FaStepBackward className="w-8 h-8" />
                </button>

                {/* Skip 10 seconds backward */}
                <button
                    className="px-2 py-2"
                    onClick={() => state.skipBackward()}
                    aria-label="Skip Backward 10s"
                >
                    <MdReplay10 className="h-10 w-10" />
                </button>

                {/* Play/Pause Button */}
                <button
                    className="rounded  px-2 py-2"
                    onClick={() => {
                        state.setPlaying(!state.playing);
                    }}
                    aria-label={state.playing ? "Pause" : "Play"}
                >
                    <Icon className="h-10 w-10"></Icon>
                </button>

                {/* Skip 10 seconds forward */}
                <button
                    className="px-2 py-2"
                    onClick={() => state.skipForward()}
                    aria-label="Skip Forward 10s"
                >
                    <MdForward10 className="h-10 w-10" />
                </button>
                {/* Skip to end */}
                <button
                    className="px-2 py-2"
                    onClick={() => state.skipToEnd()}
                    aria-label="Skip to End"
                >
                    <FaStepForward className="w-8 h-8" />
                </button>

                {/* Time display */}
                <div className="flex flex-row items-center border border-black dark:border-white py-1 px-2 rounded-2xl">
                    <span className="font-mono">{formattedTime}</span>
                    <div className="w-px h-[24px] bg-slate-300 mx-[10px]"></div>
                    <span className="font-mono">{formattedMaxTime}</span>
                </div>
            </div>
            <ScaleRangeInput
                max={state.maxTime}
                value={state.currentTimeInMs}
                onChange={(value: any) => {
                    state.handleSeek(value);
                }}
                height={32}
                markings={MARKINGS}
                backgroundColor="#2f2f2f2f"
            />
        </div>
    );
});

export default SeekPlayer