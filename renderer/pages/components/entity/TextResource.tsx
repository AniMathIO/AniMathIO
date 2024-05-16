"use client";
import React from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import { PlusCircleIcon } from "@heroicons/react/24/solid";


type TextResourceProps = {
  fontSize: number;
  fontWeight: number;
  sampleText: string;
};
const TextResource = observer(
  ({ fontSize, fontWeight, sampleText }: TextResourceProps) => {
    const state = React.useContext(StateContext);
    return (
      <div className="items-center m-[15px] flex flex-row">
        <div
          className="flex-1 text-black px-2 py-1"
          style={{
            fontSize: `${fontSize}px`,
            fontWeight: `${fontWeight}`,
          }}
        >
          {sampleText}
        </div>
        <button
          aria-label="Add Text"
          className="h-[32px] w-[32px] hover:bg-black bg-[rgba(0,0,0,.25)] rounded z-10 text-white font-bold py-1 flex items-center justify-center"
          onClick={() =>
            state.addText({
              text: sampleText,
              fontSize: fontSize,
              fontWeight: fontWeight,
            })
          }
        >
          <PlusCircleIcon className="h-6 w-6" />
        </button>
      </div>
    );
  }
);

export default TextResource;