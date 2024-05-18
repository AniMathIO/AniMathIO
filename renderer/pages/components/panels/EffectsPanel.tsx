"use client";
import React from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import EffectResource from "../entity/EffectResource";
import { isEditorImageElement, isEditorVideoElement, isEditorMafsElement } from "../../../states/state";

const EffectsPanel = observer(() => {
  const state = React.useContext(StateContext);
  const selectedElement = state.selectedElement;
  return (
    <>
      <div className="text-lg px-[16px] pt-[16px] pb-[8px] font-semibold">
        Effects
      </div>
      {selectedElement &&
        (isEditorImageElement(selectedElement) ||
          isEditorMafsElement(selectedElement) ||
          isEditorVideoElement(selectedElement)) ? (
        <EffectResource editorElement={selectedElement} />
      ) : null}
    </>
  );
});

export default EffectsPanel;
