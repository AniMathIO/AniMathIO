"use client";
import React from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import { PlusCircleIcon } from "@heroicons/react/24/solid";
import { removeBackground } from "@imgly/background-removal";
import { FaEraser } from "react-icons/fa";

type ImageResourceProps = {
  image: string;
  index: number;
};
const ImageResource = observer(
  ({ image, index }: ImageResourceProps) => {
    const state = React.useContext(StateContext);
    const ref = React.useRef<HTMLImageElement>(null);
    const [resolution, setResolution] = React.useState({ w: 0, h: 0 });
    const [isProcessing, setIsProcessing] = React.useState(false);

    const handleRemoveBackground = async () => {
      try {
        setIsProcessing(true);

        // Fetch the image first (needed for blob URLs)
        const response = await fetch(image);
        const imageBlob = await response.blob();

        // Process the image to remove background with configuration
        const resultBlob = await removeBackground(imageBlob, {
          debug: false,
          model: 'isnet_fp16', // This model offers a good balance between quality and speed
          output: {
            format: 'image/png',
            quality: 0.8
          }
        });

        // Create a new object URL from the result
        const newImageUrl = URL.createObjectURL(resultBlob);

        // Replace the image in the state
        state.replaceImageResource(index, newImageUrl);

        setIsProcessing(false);
      } catch (error) {
        console.error("Background removal failed:", error);
        setIsProcessing(false);
        alert("Failed to remove background. Please try again.");
      }
    };

    return (
      <div className="rounded-lg overflow-hidden items-center bg-slate-800 m-[15px] flex flex-col relative">
        <div className="bg-[rgba(0,0,0,.50)] rounded-xl text-white py-1 px-1 absolute text-base top-1.3 right-2">
          {resolution.w}x{resolution.h}
        </div>
        <button
          title="Add Image"
          className="hover:bg-[#00a0f5] rounded-sm z-10 text-white font-bold py-1 absolute text-lg bottom-2 right-2"
          onClick={() => state.addImage(index)}
        >
          <PlusCircleIcon className="w-8 h-8 drop-shadow-lg" />
        </button>
        <button
          title="Remove Background"
          disabled={isProcessing}
          className={`hover:bg-[#00a0f5] rounded z-10 text-white font-bold py-1 absolute text-lg bottom-12 right-2 ${isProcessing ? 'opacity-50 cursor-not-allowed' : ''
            }`}
          onClick={handleRemoveBackground}
        >
          {isProcessing ? (
            <div className="w-8 h-8 rounded-full border-4 border-t-transparent border-white animate-spin" />
          ) : (
            <FaEraser className="w-7 h-7 drop-shadow-lg" />
          )}
        </button>
        <div className="w-full flex justify-center">
          <img
            aria-label={image}
            onLoad={() => {
              setResolution({
                w: ref.current?.naturalWidth ?? 0,
                h: ref.current?.naturalHeight ?? 0,
              });
            }}
            ref={ref}
            className="min-h-[130px] min-w-[200px] max-h-[130px] max-w-[300px] object-scale-down"
            src={image}
            alt="image"
            title="image"
            id={`image-${index}`}
          ></img>
        </div>
      </div>
    );
  }
);

export default ImageResource;