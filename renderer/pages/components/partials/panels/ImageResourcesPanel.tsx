"use client";
import React from "react";
import { StateContext } from "../../../states";
import { observer } from "mobx-react";
import { ImageResource } from "../../entity/ImageResource";
import { UploadButton } from "../shared/UploadButton";

export const ImageResourcesPanel = observer(() => {
  const state = React.useContext(StateContext);
  const handleFileChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    state.addImageResource(URL.createObjectURL(file));
  };
  return (
    <>
      <div className="text-sm px-[16px] pt-[16px] pb-[8px] font-semibold">
        Images
      </div>
      <UploadButton
        accept="image/*"
        className="bg-gray-300 hover:bg-gray-400 text-gray-800 font-bold text-center mx-2 py-2 px-4 rounded cursor-pointer"
        onChange={handleFileChange}
      />
      <div >
        {state.images.map((image, index) => {
          return <ImageResource key={image} image={image} index={index} />;
        })}
      </div>

    </>
  );
});
