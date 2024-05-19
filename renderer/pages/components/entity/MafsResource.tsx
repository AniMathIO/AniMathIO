import React from "react";
import { observer } from "mobx-react";
import { PlusCircleIcon } from "@heroicons/react/24/solid";
import { MafsResourceProps } from "@/types";

const MafsResource = observer(({ index, children, name, onAddResource }: MafsResourceProps) => {
  return (
    <div className="rounded-lg overflow-hidden items-center bg-slate-800 m-[15px] flex flex-col relative">
      <div className="bg-[rgba(0,0,0,.50)] rounded-xl text-white py-1 px-1 absolute text-base top-1.3 right-2">
        {name}
      </div>
      <button
        title="Add Mafs Element"
        className="hover:bg-[#00a0f5] rounded z-10 text-white font-bold py-1 absolute text-lg bottom-2 right-2"
        onClick={onAddResource}
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

export default MafsResource;