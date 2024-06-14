"use client";
import React from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import Element from "../entity/Element";

const ElementsPanel = observer((_props: {}) => {
  const state = React.useContext(StateContext);
  return (
    <div className="bg-slate-200 text-black dark:bg-gray-700 dark:text-white h-full overflow-scroll">
      <div className="flex flex-row justify-between">
        <div className="text-lg px-[16px] pt-[16px] pb-[8px] font-semibold">Media Pool</div>
      </div>
      <div className="flex flex-col py-[15px]">
        {state.editorElements.map((element) => {
          return <Element key={element.id} element={element} />;
        })}
      </div>
    </div>
  );
});

export default ElementsPanel;
