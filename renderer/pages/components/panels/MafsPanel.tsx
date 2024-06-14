import React, { useState } from "react";
import { observer } from "mobx-react";
import { Mafs, Coordinates, Line, Point, Circle, Polygon, Text, LaTeX, Ellipse, Plot, Vector, Transform } from "mafs";
import MafsResource from "../entity/MafsResource";
import MafsModal from "../partials/MafsModal";
import { MafsResourceType } from "@/types";


const generateMafsResources = (): MafsResourceType[] => {
  const resources: MafsResourceType[] = [
    {
      index: 1,
      name: "LaTex",
      children: <LaTeX
        at={[0, 2.3]}
        tex={String.raw`
      \begin{bmatrix} ${[1, 0]} \\ ${[0, 1]} \end{bmatrix}
    `} />,
      coordinates: false,
    },
    {
      index: 2,
      name: "Text",
      children: <Text x={0} y={2.3} children="Hello, AniMathIO!" />,
      coordinates: false,
    },
    {
      index: 3,
      name: "Point",
      children: <Point x={0} y={2.3} />,
      coordinates: true,
    },
    {
      index: 4,
      name: "Line",
      children: <Line.Segment point1={[-2, 1]} point2={[2, 3]} />,
      coordinates: true,
    },
    {
      index: 5,
      name: "Vector",
      children: <Vector tail={[-1, 1]} tip={[2, 3]} />,
      coordinates: true,
    },
    {
      index: 6,
      name: "Polygon",
      children: <Polygon points={[[-1, 2], [1, 1.5], [1, 3]]} />,
      coordinates: true,
    },
    {
      index: 7,
      name: "Plot",
      children: <Plot.OfX y={(x) => Math.exp(x)} />,
      coordinates: true,
    },
    {
      index: 8,
      name: "Circle",
      children: <Circle center={[0, 2.3]} radius={1} />,
      coordinates: true,
    },
    {
      index: 9,
      name: "Ellipse",
      children: <Ellipse center={[0, 2.3]} radius={[2, 1]} />,
      coordinates: true,
    },
  ];

  return resources;
};

const MafsPanel = observer(() => {
  const mafsResources = generateMafsResources();
  const [modalOpen, setModalOpen] = useState(false);
  const [selectedResource, setSelectedResource] = useState<MafsResourceType | null>(null);

  const openModal = (resource: MafsResourceType) => {
    setSelectedResource(resource);
    setModalOpen(true);
  };

  const closeModal = () => {
    setSelectedResource(null);
    setModalOpen(false);
  };

  return (
    <>
      <div className="text-lg px-[16px] pt-[16px] pb-[15px] font-semibold">
        Mathematical Objects
      </div>
      <div className="flex bg-slate-200 dark:bg-gray-700 flex-col items-center justify-center">
        {mafsResources.map((resource) => (
          <MafsResource
            key={`mafs - ${resource.index} `}
            name={resource.name}
            onAddResource={() => openModal(resource)}
          >
            <Mafs pan={false} width={250} height={300} zoom={false}>
              {resource.coordinates ? (
                resource.coordinateType === "polar" ? (
                  <Coordinates.Polar />
                ) : (
                  <Coordinates.Cartesian />
                )
              ) : null}
              {resource.children}
            </Mafs>
          </MafsResource>
        ))}
      </div>
      {selectedResource && (
        <MafsModal
          isOpen={modalOpen}
          onClose={closeModal}
          mafsElement={selectedResource}
          className="bg-white dark:bg-gray-800 text-black dark:text-white"
        />
      )}
    </>
  );
});

export default MafsPanel;