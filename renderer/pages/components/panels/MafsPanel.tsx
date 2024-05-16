import React from "react";
import { observer } from "mobx-react";
import { Mafs, Coordinates, Line, Point, Circle, Polygon, Text, LaTeX } from "mafs";
import MafsResource from "../entity/MafsResource";

type MafsResourceType = {
  index: number
  name: string
  children: React.ReactNode
  coordinates?: true | false
  coordinateType?: "cartesian" | "polar"
}

const generateMafsResources = (): MafsResourceType[] => {
  const resources: MafsResourceType[] = [
    {
      index: 1,
      name: "Line",
      children: <Line.Segment point1={[-2, 1]} point2={[2, 3]} />,
      coordinates: false,
    },
    {
      index: 2,
      name: "Point",
      children: <Point x={0} y={2.3} />,
      coordinates: false,
    },
    {
      index: 3,
      name: "Circle",
      children: <Circle center={[0, 2.3]} radius={1} />,
      coordinates: false,
    },
    {
      index: 4,
      name: "Polygon",
      children: <Polygon points={[[-1, 2], [1, 1.5], [1, 3]]} />,
      coordinates: false,
    },
    {
      index: 5,
      name: "Text",
      children: <Text x={0} y={2} children="Hello, Mafs!" />,
      coordinates: false,
    },
    {
      index: 6,
      name: "LaTex",
      children: <LaTeX
        at={[0, 2.3]}
        tex={String.raw`
      \begin{bmatrix} ${[1, 0]} \\ ${[0, 1]} \end{bmatrix}
    `} />,
      coordinates: false,
    }
    // Add more Mafs components here
  ];

  return resources;
};



const MafsPanel = observer(() => {
  const mafsResources = generateMafsResources();
  return (
    <>
      <div className="text-lg px-[16px] pt-[16px] pb-[15px] font-semibold">
        Mathematical Objects
      </div>
      <div className="flex flex-col items-center justify-center">
        {mafsResources.map((resource) => (
          <MafsResource key={`mafs-${resource.index}`} index={resource.index} name={resource.name}>
            <Mafs pan={false} width={250} height={300} zoom={false}>
              {resource.coordinates ? resource.coordinateType === "cartesian" ? <Coordinates.Cartesian /> : <Coordinates.Polar /> : null}
              {resource.children}
            </Mafs>
          </MafsResource>
        ))}
      </div >
    </>
  );
});

export default MafsPanel;