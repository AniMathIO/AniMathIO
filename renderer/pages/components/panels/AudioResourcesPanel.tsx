"use client";
import React from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import AudioResource from "../entity/AudioResource";
import UploadButton from "../partials/shared/UploadButton";

const AudioResourcesPanel = observer(() => {
  const state = React.useContext(StateContext);
  const handleFileChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    state.addAudioResource(URL.createObjectURL(file));
  };
  return (
    <>
      <div className="text-lg px-[16px] pt-[16px] pb-[15px] font-semibold">
        Audio Resources
      </div>
      {state.audios.map((audio, index) => {
        return <AudioResource key={audio} audio={audio} index={index} />;
      })}
      <UploadButton
        accept="audio/mp3,audio/*"
        className="ml-[15px] bg-gray-300 hover:bg-gray-400 text-gray-800 font-bold text-center mx-2 py-2 px-4 rounded cursor-pointer"
        onChange={handleFileChange}
      />
    </>
  );
});

export default AudioResourcesPanel;
