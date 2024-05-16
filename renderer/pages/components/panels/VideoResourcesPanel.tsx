"use client";
import React from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import VideoResource from "../entity/VideoResource";
import UploadButton from "../partials/shared/UploadButton";

const VideoResourcesPanel = observer(() => {
  const state = React.useContext(StateContext);
  const handleFileChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    state.addVideoResource(URL.createObjectURL(file));
  };
  return (
    <>
      <div className="text-lg px-[16px] pt-[16px] pb-[15px] font-semibold">
        Video Resources
      </div>
      {state.videos.map((video, index) => {
        return <VideoResource key={video} video={video} index={index} />;
      })}
      <UploadButton
        accept="video/mp4,video/x-m4v,video/*"
        className="ml-[15px]  bg-gray-300 hover:bg-gray-400 text-gray-800 font-bold text-center mx-5 py-2 px-4 rounded cursor-pointer"
        onChange={handleFileChange}
      />
    </>
  );
});

export default VideoResourcesPanel;