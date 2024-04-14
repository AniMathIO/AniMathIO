"use client";

import { StateContext } from "../../../states";
import { formatTimeToMinSecMili } from "../../../utils";
import { observer } from "mobx-react-lite";
import { useContext } from "react";
import { PlayIcon } from "@heroicons/react/24/solid";
import { PauseIcon } from "@heroicons/react/24/solid";
import { ScaleRangeInput } from "./ScaleRangeInput";



const MARKINGS = [
    {
        interval: 5000,
        color: 'black',
        size: 16,
        width: 1
    },
    {
        interval: 1000,
        color: 'black',
        size: 8,
        width: 1
    }
];

export type SeekPlayerProps = {};


export const SeekPlayer = observer((_props: SeekPlayerProps) => {
    const state = useContext(StateContext);
    const Icon = state.playing ? PauseIcon : PlayIcon;
    const formattedTime = formatTimeToMinSecMili(state.currentTimeInMs);
    const formattedMaxTime = formatTimeToMinSecMili(state.maxTime);
    return (
        <div className="seek-player flex flex-col">
            <div className="flex flex-row items-center px-2">
                <button
                    className="w-[80px] rounded  px-2 py-2"
                    onClick={() => {
                        state.setPlaying(!state.playing);
                    }}
                    aria-label={state.playing ? "Pause" : "Play"}
                >
                    <Icon className="h-10 w-10"></Icon>
                </button>
                <span className="font-mono">{formattedTime}</span>
                <div className="w-[1px] h-[25px] bg-slate-300 mx-[10px]"></div>
                <span className="font-mono">{formattedMaxTime}</span>
            </div>
            <ScaleRangeInput
                max={state.maxTime}
                value={state.currentTimeInMs}
                onChange={(value) => {
                    state.handleSeek(value);
                }}
                height={30}
                markings={MARKINGS}
                backgroundColor="white"
            />
        </div>
    );
});