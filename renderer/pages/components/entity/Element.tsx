"use client";
import React from "react";
import { EditorElement } from "@/types";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import { Bars3BottomLeftIcon, FilmIcon, PhotoIcon, MusicalNoteIcon, XMarkIcon, VariableIcon } from "@heroicons/react/24/solid";
import dynamic from "next/dynamic";
export type ElementProps = {
  element: EditorElement;
};

const Element = observer((props: ElementProps) => {
  const state = React.useContext(StateContext);
  const { element } = props;
  let Icon = Bars3BottomLeftIcon;
  if (element.type === "video") {
    Icon = FilmIcon;
  } else if (element.type === "image") {
    Icon = PhotoIcon;
  } else if (element.type === "audio") {
    Icon = MusicalNoteIcon;
  }
  else if (element.type === "mafs") {
    Icon = VariableIcon;
  }

  const isSelected = state.selectedElement?.id === element.id;
  const bgColor = isSelected ? "rgba(0, 155, 245, 0.336)" : "";
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
        {element.type === "mafs" ? (
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
      </div>
      <button
        aria-label="Remove element"
        className="bg-red-500 hover:bg-red-700 text-white mr-2 text-sm py-1 px-1 rounded-sm"
        onClick={(e) => {
          state.removeEditorElement(element.id);
          state.refreshElements();
          e.preventDefault();
          e.stopPropagation();
        }}
      >
        <XMarkIcon className="w-4 h-4" />
      </button>
    </div>
  );
});

export default dynamic(() => Promise.resolve(Element), { ssr: false });