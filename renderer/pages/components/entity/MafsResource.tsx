import React, { useRef } from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import { PlusCircleIcon } from "@heroicons/react/24/solid";
import * as htmlToImage from 'html-to-image';

type MafsResourceProps = {
  index: number;
  children: React.ReactNode;
  name: string;
};

const MafsResource = observer(({ index, children, name }: MafsResourceProps) => {
  const state = React.useContext(StateContext);
  const mafsRef = useRef<HTMLDivElement>(null);

  const handleAddResource = async () => {
    try {
      const pngSrc = await extractPNG();
      state.addMafsResource(index, pngSrc, name).then(() => {
        console.log("Mafs resource added successfully");
      });
      // console.log("Mafs resource added successfully");
    } catch (error) {
      console.error('Failed to extract PNG from Mafs component:', error);
    }
  };

  const extractPNG = async (): Promise<string> => {
    try {
      const dataUrl = await htmlToImage.toPng(mafsRef.current as HTMLElement, {
        filter: (node) => node.tagName !== 'I'
      });
      return dataUrl;
    } catch (error) {
      console.error('Failed to generate PNG data URL', error);
      throw new Error('Failed to generate PNG data URL');
    }
  };



  return (
    <div className="rounded-lg overflow-hidden items-center bg-slate-800 m-[15px] flex flex-col relative">
      <div className="bg-[rgba(0,0,0,.50)] rounded-xl text-white py-1 px-1 absolute text-base top-1.3 right-2">
        {name}
      </div>
      <button
        title="Add Image"
        className="hover:bg-[#00a0f5] rounded z-10 text-white font-bold py-1 absolute text-lg bottom-2 right-2"
        onClick={handleAddResource}
      >
        <PlusCircleIcon className="w-8 h-8 drop-shadow-lg" />
      </button>
      <div className="w-full flex justify-center">
        <div
          ref={mafsRef}
          id={`mafs-container-${index}`}
          className="min-h-[130px] min-w-[200px] max-h-[130px] max-w-[300px] object-scale-down"
        >
          {children}
        </div>
      </div>
    </div>
  );
});

export default MafsResource;