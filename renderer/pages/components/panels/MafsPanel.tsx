import React from "react";
import { observer } from "mobx-react";
import { Mafs, Coordinates, Line, Point } from "mafs";
import { MafsResource } from "../entity/MafsResource";

export const MafsPanel = observer(() => {
  return (
    <>
      <div className="text-lg px-[16px] pt-[16px] pb-[15px] font-semibold">
        Mathematical Objects
      </div>
      <div className="flex flex-col items-center justify-center">
        <MafsResource index={0}>
          <Mafs>
            <Coordinates.Cartesian />
            <Line.Segment point1={[-2, -1]} point2={[2, 1]} />
          </Mafs>
        </MafsResource>
        <MafsResource index={1}>
          <Mafs>
            <Coordinates.Cartesian />
            <Point x={1} y={1} />
          </Mafs>
        </MafsResource>
        {/* Add more MafsResource components with different Mafs elements */}
      </div>
    </>
  );
});