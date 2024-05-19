import React, { useState, useRef, useContext } from "react";
import { observer } from "mobx-react";
import {
    Mafs,
    Coordinates, Line,
    Point, Circle, Polygon, Text,
    LaTeX, Ellipse, Plot, Vector, CircleProps, SegmentProps, EllipseProps, PointProps, vec,
    TextProps, VectorProps
} from "mafs";
import { StateContext } from "@/states";
import * as htmlToImage from 'html-to-image';
import { MafsModalProps, LatexProps } from "@/types";

import LineInputs from "./mafsmodal/LineInputs";
import PointInputs from "./mafsmodal/PointInputs";
import CircleInputs from "./mafsmodal/CircleInputs";
import PolygonInputs from "./mafsmodal/PolygonInputs";
import TextInputs from "./mafsmodal/TextInputs";
import LatexInputs from "./mafsmodal/LatexInputs";
import EllipseInputs from "./mafsmodal/EllipseInputs";
import PlotInputs from "./mafsmodal/PlotInputs";
import VectorInputs from "./mafsmodal/VectorInputs";

const MafsModal = observer(({ isOpen, onClose, mafsElement }: MafsModalProps) => {
    const [mafsConfig, setMafsConfig] = useState({
        width: 500,
        height: 200,
        pan: true,
        zoom: true,
        coordinateType: mafsElement.coordinateType || "cartesian",
    });

    const [lineProps, setLineProps] = useState<SegmentProps>({
        point1: [-2, 1],
        point2: [2, 3],
    });

    const [ellipseProps, setEllipseProps] = useState<EllipseProps>({
        center: [0, 2.3],
        radius: [2, 1],
    });

    const [circleProps, setCircleProps] = useState<CircleProps>({
        center: [0, 2.3],
        radius: 1,
    });

    const [latexProps, setLatexProps] = useState<LatexProps>({
        at: [0, 2.3],
        tex: String.raw`\begin{bmatrix} ${[1, 0]} \\ ${[0, 1]} \end{bmatrix}`,
    });

    const [latexInput, setLatexInput] = useState(latexProps.tex);

    const [plotProps, setPlotProps] = useState({
        y: (x: number) => Math.exp(x),
    });
    const [plotInput, setPlotInput] = useState(plotProps.y.toString());

    const [pointProps, setPointProps] = useState<PointProps>({
        x: 0,
        y: 2.3,
    });

    const [polygonPoints, setPolygonPoints] = useState<vec.Vector2[]>([
        [-1, 2],
        [1, 1.5],
        [1, 3],
    ]);

    const [textProps, setTextProps] = useState<TextProps>({
        x: 0,
        y: 2.3,
        children: "Hello, AniMathIO!",
    });

    const [vectorProps, setVectorProps] = useState<VectorProps>({
        tail: [-1, 1],
        tip: [2, 3],
    });


    const handlePlotSubmit = () => {
        try {
            const newPlotFunction = eval(plotInput);
            setPlotProps({
                ...plotProps,
                y: newPlotFunction,
            });
        } catch (error) {
            console.error('Invalid plot function:', error);
            alert('Invalid plot function. Please enter a valid function.');
        }
    };

    const handleLatexSubmit = () => {
        // console.log(latexInput);
        setLatexProps((prevProps) => ({
            ...prevProps,
            tex: latexInput,
        }));
        // console.log("state set");
        // console log div ref mafs element
        // console.log(mafsRef.current);
    };

    const state = useContext(StateContext);
    const mafsRef = useRef<HTMLDivElement>(null);

    if (!isOpen) return null;

    const handleMafsConfigChange = (key: keyof typeof mafsConfig, value: any) => {
        setMafsConfig((prevConfig) => ({
            ...prevConfig,
            [key]: value,
        }));
    };

    const handleAddResource = async () => {
        try {
            const pngSrc = await extractPNG();
            state.addMafsResource(mafsElement.index, pngSrc, mafsElement.name).then(() => {
                console.log("Mafs resource added successfully");
                onClose();
            });
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

    const renderMafsElement = () => {
        // console.log(mafsElement.name);
        // console.log(latexProps.tex);
        switch (mafsElement.name) {
            case "Line":
                return <Line.Segment {...lineProps} />;
            case "Point":
                return <Point {...pointProps} />;
            case "Circle":
                return <Circle {...circleProps} />;
            case "Polygon":
                return <Polygon points={polygonPoints} />;
            case "Text":
                return <Text {...textProps} />;
            case "LaTex":
                return <LaTeX at={latexProps.at} tex={latexProps.tex} />;
            case "Ellipse":
                return <Ellipse {...ellipseProps} />;
            case "Plot":
                return <Plot.OfX {...plotProps} />;
            case "Vector":
                return <Vector {...vectorProps} />;
            default:
                return null;
        }
    };

    return (
        <div className="fixed inset-0 z-50 overflow-auto bg-black bg-opacity-50 flex items-start justify-center pt-16">
            <div className="bg-white rounded-lg shadow-lg p-6 w-full max-w-4xl max-h-[calc(100vh-8rem)] overflow-auto">
                <div className="flex justify-between items-center mb-4">
                    <h2 className="text-2xl font-bold">{mafsElement.name}</h2>
                    <button
                        className="text-gray-500 hover:text-gray-700"
                        onClick={onClose}
                    >
                        <span className="sr-only">Close</span>
                        <svg
                            className="h-6 w-6"
                            xmlns="http://www.w3.org/2000/svg"
                            fill="none"
                            viewBox="0 0 24 24"
                            stroke="currentColor"
                            aria-hidden="true"
                        >
                            <path
                                strokeLinecap="round"
                                strokeLinejoin="round"
                                strokeWidth={2}
                                d="M6 18L18 6M6 6l12 12"
                            />
                        </svg>
                    </button>
                </div>
                <div className="mb-4">
                    <label htmlFor="width" className="block font-medium mb-1">
                        Width
                    </label>
                    <input
                        type="number"
                        id="width"
                        max={835}
                        maxLength={3}
                        className="w-full border border-gray-300 rounded px-3 py-2"
                        value={mafsConfig.width}
                        onChange={(e) =>
                            handleMafsConfigChange("width", parseInt(e.target.value))
                        }
                    />
                </div>
                <div className="mb-4">
                    <label htmlFor="height" className="block font-medium mb-1">
                        Height
                    </label>
                    <input
                        type="number"
                        id="height"
                        max={235}
                        maxLength={3}
                        className="w-full border border-gray-300 rounded px-3 py-2"
                        value={mafsConfig.height}
                        onChange={(e) =>
                            handleMafsConfigChange("height", parseInt(e.target.value))
                        }
                    />
                </div>
                <div className="mb-4">
                    <label className="inline-flex items-center">
                        <input
                            type="checkbox"
                            checked={mafsConfig.pan}
                            onChange={(e) => handleMafsConfigChange("pan", e.target.checked)}
                            className="form-checkbox"
                        />
                        <span className="ml-2">Pan</span>
                    </label>
                </div>
                <div className="mb-4">
                    <label className="inline-flex items-center">
                        <input
                            type="checkbox"
                            checked={mafsConfig.zoom}
                            onChange={(e) => handleMafsConfigChange("zoom", e.target.checked)}
                            className="form-checkbox"
                        />
                        <span className="ml-2">Zoom</span>
                    </label>
                </div>
                <div className="mb-4">
                    <label htmlFor="coordinateType" className="block font-medium mb-1">
                        Coordinate Type
                    </label>
                    <select
                        id="coordinateType"
                        className="w-full border border-gray-300 rounded px-3 py-2"
                        value={mafsConfig.coordinateType}
                        onChange={(e) =>
                            handleMafsConfigChange("coordinateType", e.target.value)
                        }
                    >
                        <option value="cartesian">Cartesian</option>
                        <option value="polar">Polar</option>
                        <option value="none">None</option>

                    </select>
                </div>
                {mafsElement.name === "Line" && <LineInputs lineProps={lineProps} setLineProps={setLineProps} />}
                {mafsElement.name === "Point" && <PointInputs pointProps={pointProps} setPointProps={setPointProps} />}
                {mafsElement.name === "Circle" && <CircleInputs circleProps={circleProps} setCircleProps={setCircleProps} />}
                {mafsElement.name === "Polygon" && <PolygonInputs polygonPoints={polygonPoints} setPolygonPoints={setPolygonPoints} />}
                {mafsElement.name === "Text" && <TextInputs textProps={textProps} setTextProps={setTextProps} />}
                {mafsElement.name === "LaTex" && <LatexInputs latexProps={latexProps} setLatexProps={setLatexProps} latexInput={latexInput} setLatexInput={setLatexInput} handleLatexSubmit={handleLatexSubmit} />}
                {mafsElement.name === "Ellipse" && <EllipseInputs ellipseProps={ellipseProps} setEllipseProps={setEllipseProps} />}
                {mafsElement.name === "Plot" && <PlotInputs plotProps={plotProps} setPlotProps={setPlotProps} plotInput={plotInput} setPlotInput={setPlotInput} handlePlotSubmit={handlePlotSubmit} />}
                {mafsElement.name === "Vector" && <VectorInputs vectorProps={vectorProps} setVectorProps={setVectorProps} />}
                <div className="border border-gray-300 rounded p-4 flex justify-center">
                    <div ref={mafsRef}>
                        <Mafs width={mafsConfig.width} height={mafsConfig.height} pan={mafsConfig.pan} zoom={mafsConfig.zoom} >
                            {mafsConfig.coordinateType === "cartesian" && <Coordinates.Cartesian />}
                            {mafsConfig.coordinateType === "polar" && <Coordinates.Polar />}
                            {renderMafsElement()}
                        </Mafs>
                    </div>
                </div>
                <div className="mt-4 flex justify-end">
                    <button className="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded mr-2" onClick={handleAddResource} >
                        Add Element
                    </button>
                </div>
            </div>
        </div >
    );
});

export default MafsModal;