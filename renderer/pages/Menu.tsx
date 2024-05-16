"use client";
import React from "react";
import { StateContext } from "../states";
import { observer } from "mobx-react";
import {
  ArrowDownTrayIcon,
  FilmIcon,
  PhotoIcon,
  ArrowsPointingOutIcon,
  DocumentTextIcon,
  MusicalNoteIcon,
  EyeDropperIcon,
  SparklesIcon,
  VariableIcon
} from "@heroicons/react/24/solid";
import { State } from "../states/state";

const Menu = observer(() => {
  const state = React.useContext(StateContext);

  return (
    <ul className="bg-white h-full flex flex-col justify-start items-center">
      {MENU_OPTIONS.map((option) => {
        const isSelected = state.selectedMenuOption === option.name;
        return (
          <li
            key={option.name}
            className={`h-[72px] w-[72px] flex flex-col items-center justify-center ${isSelected ? "bg-slate-200" : ""}`}
          >
            <button
              onClick={() => option.action(state)}
              className={`flex flex-col items-center`}
            >
              <option.icon
                className="h-10 w-10"
                color={
                  isSelected ? "#000" : "#444"
                }
              />
              <div
                className={`text-[0.6rem] hover:text-black ${isSelected ? "text-black" : "text-slate-600"}`}
              >
                {option.name}
              </div>
            </button>
          </li>
        );
      })}
    </ul>
  );
});

const MENU_OPTIONS = [
  {
    name: "Videos",
    icon: FilmIcon,
    action: (state: State) => {
      state.setSelectedMenuOption("Videos");
    },
  },
  {
    name: "Images",
    icon: PhotoIcon,
    action: (state: State) => {
      state.setSelectedMenuOption("Images");
    },
  },
  {
    name: "Mathematical Objects",
    icon: VariableIcon,
    action: (state: State) => {
      state.setSelectedMenuOption("Mathematical Objects");
    },
  },
  {
    name: "Audios",
    icon: MusicalNoteIcon,
    action: (state: State) => {
      state.setSelectedMenuOption("Audios");
    },
  },
  {
    name: "Text",
    icon: DocumentTextIcon,
    action: (state: State) => {
      state.setSelectedMenuOption("Text");
    },
  },
  {
    name: "Animations",
    icon: ArrowsPointingOutIcon,
    action: (state: State) => {
      state.setSelectedMenuOption("Animations");
    },
  },
  {
    name: "Effects",
    icon: SparklesIcon,
    action: (state: State) => {
      state.setSelectedMenuOption("Effects");
    },
  },
  {
    name: "Background Fill",
    icon: EyeDropperIcon,
    action: (state: State) => {
      state.setSelectedMenuOption("Background Fill");
    },
  },
  {
    name: "Export",
    icon: ArrowDownTrayIcon,
    action: (state: State) => {
      state.setSelectedMenuOption("Export");
    },
  },
];

export default Menu;