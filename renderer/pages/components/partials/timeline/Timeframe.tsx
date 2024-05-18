"use client";
import React from "react";
import { EditorElement } from "@/types";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import Dragable from "./Dragable";
import WavesurferPlayer from "@wavesurfer/react";
import dynamic from "next/dynamic";

const TimeFrame = observer((props: { element: EditorElement }) => {
    const state = React.useContext(StateContext);
    const { element } = props;
    const disabled = element.type === "audio";
    const isSelected = state.selectedElement?.id === element.id;
    const bgColorOnSelected = isSelected ? "bg-slate-800" : "bg-slate-600";
    const disabledCursor = disabled ? "cursor-no-drop" : "cursor-ew-resize";

    const renderElementContent = (element: EditorElement) => {
        const commonClasses = "w-full mt-[calc(-64px/2)] relative z-1";
        switch (element.type) {
            case "audio":
                return (
                    <div className={`${commonClasses} pt-[14px]`}>
                        <WavesurferPlayer
                            url={element.properties.src}
                            height={48}
                            width={`100%`}
                            autoCenter={true}
                            autoScroll={true}
                            waveColor="#00a0f5"
                            progressColor="#027bbca9"
                            normalize={true}
                            interact={false}
                            cursorWidth={0}
                            fillParent={true}
                        // TODO: implement cursor based on the current time
                        // onReady={onReady}
                        // onPlay={() => setIsPlaying(true)}
                        // onPause={() => setIsPlaying(false)}
                        />
                    </div>
                );
            case "image":
                return (
                    <div className={`${commonClasses} py-1`}>
                        <img
                            alt="image"
                            src={element.properties.src}
                            className="h-[64px]"
                        />
                    </div>
                );
            case "video":
                return (
                    <div className={`${commonClasses} py-1`}>
                        <video
                            src={element.properties.src}
                            className="h-[64px]"
                        />
                    </div>
                );
            case "text":
                return (
                    <div className={`${commonClasses} flex flex-col justify-end h-[64px]`}>
                        <p className="text-4xl">{element.properties.text}</p>
                    </div>
                );
            case "mafs":
                return (
                    <div className={`${commonClasses} py-1`}>
                        <img
                            alt="mafs"
                            src={element.properties.src}
                            className="h-[64px]"
                        />
                    </div>
                );
            default:
                return null;
        }
    };


    return (
        <div
            onClick={() => {
                state.setSelectedElement(element);
            }}
            key={element.id}
            className={`relative width-full h-[64px] my-2 z-2  ${isSelected ? "border-2 border-indigo-600 bg-slate-200" : ""
                }`}
        >
            <Dragable
                className="z-10"
                value={element.timeFrame.start}
                total={state.maxTime}
                disabled={disabled}
                onChange={(value) => {
                    state.updateEditorElementTimeFrame(element, {
                        start: value,
                    });
                }}
            >
                <div
                    className={`bg-white border-2 border-blue-400 w-[10px] h-[64px] mt-[calc(64px/2)]  translate-y-[-50%] transform translate-x-[-50%] ${disabledCursor}`}
                ></div>
            </Dragable>

            <Dragable
                className={disabled ? "cursor-no-drop" : "cursor-col-resize"}
                value={element.timeFrame.start}
                disabled={disabled}
                style={{
                    width: `${((element.timeFrame.end - element.timeFrame.start) /
                        state.maxTime) *
                        100
                        }%`,
                }}
                total={state.maxTime}
                onChange={(value) => {
                    const { start, end } = element.timeFrame;
                    state.updateEditorElementTimeFrame(element, {
                        start: value,
                        end: value + (end - start),
                    });
                }}
            >
                <div
                    className={`${bgColorOnSelected} h-full w-full text-white text-xs min-w-[0px] px-2 leading-[25px] overflow-hidden`}
                >
                    {/* fet element name on top of wavesurfer */}
                    <div className="w-28 top-0 left-0 relative z-20 rounded-b-xl px-2 bg-[rgba(0,0,0,.80)]">
                        {element.name}
                    </div>

                    {renderElementContent(element)}

                </div>
            </Dragable >
            <Dragable
                className="z-10"
                disabled={disabled}
                value={element.timeFrame.end}
                total={state.maxTime}
                onChange={(value) => {
                    state.updateEditorElementTimeFrame(element, {
                        end: value,
                    });
                }}
            >
                <div
                    className={`bg-white border-2 border-blue-400 w-[10px] h-[64px] mt-[calc(64px/2)] translate-y-[-50%] transform translate-x-[-50%] ${disabledCursor}`}
                ></div>
            </Dragable>
        </div >
    );
});

export default dynamic(() => Promise.resolve(TimeFrame), { ssr: false });