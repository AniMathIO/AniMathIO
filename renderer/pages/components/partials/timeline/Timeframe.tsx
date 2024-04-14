"use client";
import React from "react";
import { EditorElement } from "../../../../types";
import { StateContext } from "../../../states";
import { observer } from "mobx-react";
import Dragable from "./Dragable";

export const TimeFrame = observer((props: { element: EditorElement }) => {
    const state = React.useContext(StateContext);
    const { element } = props;
    const disabled = element.type === "audio";
    const isSelected = state.selectedElement?.id === element.id;
    const bgColorOnSelected = isSelected ? "bg-slate-800" : "bg-slate-600";
    const disabledCursor = disabled ? "cursor-no-drop" : "cursor-ew-resize";

    return (
        <div
            onClick={() => {
                state.setSelectedElement(element);
            }}
            key={element.id}
            className={`relative width-full h-[25px] my-2 ${isSelected ? "border-2 border-indigo-600 bg-slate-200" : ""
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
                    className={`bg-white border-2 border-blue-400 w-[10px] h-[10px] mt-[calc(25px/2)] translate-y-[-50%] transform translate-x-[-50%] ${disabledCursor}`}
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
                    className={`${bgColorOnSelected} h-full w-full text-white text-xs min-w-[0px] px-2 leading-[25px]`}
                >
                    {element.name}
                </div>
            </Dragable>
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
                    className={`bg-white border-2 border-blue-400 w-[10px] h-[10px] mt-[calc(25px/2)] translate-y-[-50%] transform translate-x-[-50%] ${disabledCursor}`}
                ></div>
            </Dragable>
        </div>
    );
});