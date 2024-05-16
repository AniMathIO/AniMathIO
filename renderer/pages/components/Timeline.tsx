"use client";
import React from "react";
import SeekPlayer from "./partials/timeline/SeekPlayer";
import { StateContext } from "../../states";
import { observer } from "mobx-react";
import TimeFrame from "./partials/timeline/Timeframe";

const Timeline = observer(() => {
    const state = React.useContext(StateContext);
    const percentOfCurrentTime = (state.currentTimeInMs / state.maxTime) * 100;
    return (
        <div className="flex flex-col overflow-hidden">
            <SeekPlayer />
            <div className="flex-1 relative ">
                {state.editorElements.map((element) => {
                    return <TimeFrame key={element.id} element={element} />;
                })}
                <div
                    className="w-[2px] bg-red-400 absolute top-0 bottom-0 z-20"
                    style={{
                        left: `${percentOfCurrentTime}%`,
                    }}
                ></div>
            </div>
        </div>
    );
});

export default Timeline;