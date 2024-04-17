import React from "react";
import { StateContext } from "../../states";
import { observer } from "mobx-react";
import { PlusCircleIcon } from "@heroicons/react/24/solid";

type MafsResourceProps = {
  index: number;
  children: React.ReactNode;
};

export const MafsResource = observer(({ index, children }: MafsResourceProps) => {
  const state = React.useContext(StateContext);

  return (
    <div className="rounded-lg overflow-hidden items-center bg-slate-800 m-[15px] flex flex-col relative">
      <div className="bg-[rgba(0,0,0,.50)] rounded-xl text-white py-1 px-1 absolute text-base top-1.3 right-2">
        Mafs Object Name
      </div>
      <button
        title="Add Image"
        className="hover:bg-[#00a0f5] rounded z-10 text-white font-bold py-1 absolute text-lg bottom-2 right-2"
        onClick={() => state.addImage(index)}
      >
        <PlusCircleIcon className="w-8 h-8 drop-shadow-lg" />
      </button>
      <div className="w-full flex justify-center">
        <div className="min-h-[130px] min-w-[200px] max-h-[130px] max-w-[300px] object-scale-down">
          {children}
        </div>
      </div>
    </div>
  );
});