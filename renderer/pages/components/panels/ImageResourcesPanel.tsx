"use client";
import React from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import ImageResource from "../entity/ImageResource";
import UploadButton from "../partials/shared/UploadButton";

const ImageResourcesPanel = observer(() => {
  const state = React.useContext(StateContext);
  const handleFileChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    state.addImageResource(URL.createObjectURL(file));
  };
  return (
    <div className="bg-slate-200 dark:bg-gray-700 ">
      <div className="text-lg px-[16px] pt-[16px] pb-[15px] font-semibold">
        Image Resources
      </div>

      <div className="" >
        {state.images.map((image, index) => {
          return <ImageResource key={image} image={image} index={index} />;
        })}
      </div>
      <UploadButton
        accept="image/*"
        className="ml-[15px] bg-gray-300 hover:bg-gray-400 text-gray-800 font-bold text-center mx-5 py-2 px-4 rounded-sm cursor-pointer"
        onChange={handleFileChange}
      />

    </div >
  );
});

export default ImageResourcesPanel;