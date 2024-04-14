"use client";
import React from "react";
import { EditorElement } from "../../../types";
import { StateContext } from "../../states";
import { observer } from "mobx-react";
import { Bars3BottomLeftIcon, FilmIcon } from "@heroicons/react/24/solid";

export type ElementProps = {
  element: EditorElement;
};

export const Element = observer((props: ElementProps) => {
  const state = React.useContext(StateContext);
  const { element } = props;
  const Icon = element.type === "video" ? FilmIcon : Bars3BottomLeftIcon;
  const isSelected = state.selectedElement?.id === element.id;
  const bgColor = isSelected ? "rgba(0, 160, 245, 0.1)" : "";
  return (
    <div
      style={{
        backgroundColor: bgColor,
      }}
      className={`flex mx-2 my-1 py-2 px-1 flex-row justify-start items-center ${bgColor}`}
      key={element.id}
      onClick={() => {
        state.setSelectedElement(element);
      }}
    >
      <Icon className="w-5 h-5" />
      <div className="truncate text-xs ml-2 flex-1 font-medium">
        {element.name}
      </div>
      <div>
        {element.type === "video" ? (
          <video
            className="opacity-0 max-w-[20px] max-h-[20px]"
            src={element.properties.src}
            onLoad={() => {
              state.refreshElements();
            }}
            onLoadedData={() => {
              state.refreshElements();
            }}
            height={20}
            width={20}
            id={element.properties.elementId}
          ></video>
        ) : null}
        {element.type === "image" ? (
          <img
            alt={element.name}
            className="opacity-0 max-w-[20px] max-h-[20px]"
            src={element.properties.src}
            onLoad={() => {
              state.refreshElements();
            }}
            onLoadedData={() => {
              state.refreshElements();
            }}
            height={20}
            width={20}
            id={element.properties.elementId}
          ></img>
        ) : null}
        {element.type === "audio" ? (
          <audio
            className="opacity-0 max-w-[20px] max-h-[20px]"
            src={element.properties.src}
            onLoad={() => {
              state.refreshElements();
            }}
            onLoadedData={() => {
              state.refreshElements();
            }}
            id={element.properties.elementId}
          ></audio>
        ) : null}
      </div>
      <button
        className="bg-red-500 hover:bg-red-700 text-white mr-2 text-xs py-0 px-1 rounded"
        onClick={(e) => {
          state.removeEditorElement(element.id);
          state.refreshElements();
          e.preventDefault();
          e.stopPropagation();
        }}
      >
        X
      </button>
    </div>
  );
});
