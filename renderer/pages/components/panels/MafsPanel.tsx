"use client";
import React from "react";
// import { StateContext } from "../../states";
import { observer } from "mobx-react";
import { Mafs, Coordinates, Text, useMovablePoint } from "mafs"



export const MafsPanel = observer(() => {
  const point = useMovablePoint([1, 1])
  return (
    <>
      <div className="text-lg px-[16px] pt-[16px] pb-[15px] font-semibold">
        Mathematical Objects
      </div>
      <div className="flex items-center justify-center ">
        <Mafs viewBox={{ y: [0, 2], x: [-3, 5] }}>
          <Coordinates.Cartesian />
          <Text
            x={point.x}
            y={point.y}
            size={20}
            attach="w"
            attachDistance={15}
          >
            ({point.x.toFixed(3)}, {point.y.toFixed(3)})
          </Text>
          <Text
            x={point.x}
            y={point.y}
            size={20}
            attach="e"
            attachDistance={15}
          >
            ({point.x.toFixed(3)}, {point.y.toFixed(3)})
          </Text>
          {point.element}
        </Mafs>

      </div >
    </>
  );
});
