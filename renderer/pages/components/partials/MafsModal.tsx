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
import { applyMafsCaptureStyles } from "@/utils/mafs-capture";
import katex from "katex";
import Modal from 'react-modal';
import dynamic from "next/dynamic";

import LineInputs from "./mafsmodal/LineInputs";
import PointInputs from "./mafsmodal/PointInputs";
import CircleInputs from "./mafsmodal/CircleInputs";
import PolygonInputs from "./mafsmodal/PolygonInputs";
import TextInputs from "./mafsmodal/TextInputs";
import LatexInputs from "./mafsmodal/LatexInputs";
import EllipseInputs from "./mafsmodal/EllipseInputs";
import PlotInputs from "./mafsmodal/PlotInputs";
import VectorInputs from "./mafsmodal/VectorInputs";

const MafsModal = observer(({ isOpen, onClose, mafsElement, className }: MafsModalProps) => {
    const [mafsConfig, setMafsConfig] = useState({
        width: 500,
        height: 200,
        pan: true,
        zoom: true,
        coordinateType: mafsElement.coordinateType || "cartesian",
    });


    const [isErrorModalOpen, setIsErrorModalOpen] = useState(false);
    const [errorMessage, setErrorMessage] = useState('');

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
            const supportedMathFunctions = ['Math.sin', 'Math.cos', 'Math.tan', 'Math.exp', 'Math.log', 'Math.sqrt'];
            const hasMathFunction = supportedMathFunctions.some(func => plotInput.includes(func));
            if (!hasMathFunction) {
                throw new Error('Invalid plot function');
            }
            const newPlotFunction = eval(plotInput);
            setPlotProps({
                ...plotProps,
                y: newPlotFunction,
            });
        } catch (error) {
            console.error('Invalid plot function:', error);
            setErrorMessage('Invalid plot function. Please enter a valid function using Math.sin, Math.cos, Math.tan, Math.exp, Math.log, or Math.sqrt.');
            setIsErrorModalOpen(true);
        }
    };

    const handleLatexSubmit = () => {
        try {
            katex.renderToString(latexInput);
            setLatexProps((prevProps) => ({
                ...prevProps,
                tex: latexInput,
            }));
        } catch (error) {
            console.error('Invalid LaTeX string:', error);
            setErrorMessage('Invalid LaTeX string. Please enter a valid LaTeX expression.');
            setIsErrorModalOpen(true);
        }
    };

    const state = useContext(StateContext);
    const mafsRef = useRef<HTMLDivElement>(null);

    if (!isOpen) return null;

    const handleMafsConfigChange = (key: keyof typeof mafsConfig, value: any) => {
        try {
            setMafsConfig((prevConfig) => ({
                ...prevConfig,
                [key]: value,
            }));
        } catch (error) {
            console.error('Failed to update Mafs config:', error);
            setErrorMessage('An error occurred while updating the Mafs configuration. Please try again.');
            setIsErrorModalOpen(true);
        }
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
            setErrorMessage('An error occurred while adding the Mafs resource. Please try again.');
            setIsErrorModalOpen(true);
        }
    };

    const extractPNG = async (): Promise<string> => {
        try {
            const container = mafsRef.current as HTMLElement;
            // Mafs's axis/grid colors are CSS custom properties declared only in
            // the `.MafsView { ... }` stylesheet rule (core.css + our globals.css
            // override). html-to-image clones the DOM verbatim for SVG content —
            // it never embeds that rule into its isolated capture document (it
            // lives inside a Tailwind v4 `@layer` block html-to-image's stylesheet
            // walker doesn't recurse into), so every `var(--mafs-*)` reference in
            // the cloned SVG's `stroke`/`fill` attributes stays unresolved and
            // renders invisible — axis lines, grid, and tick numbers vanish even
            // though they're present and visible in the live DOM. Custom
            // properties set as an inline `style` attribute, by contrast, *do*
            // survive cloneNode verbatim, so we set every property the axis/grid/
            // label rendering depends on directly here (mirroring core.css's
            // defaults) instead of relying on the stylesheet cascade at capture
            // time. --mafs-fg (and --mafs-origin-color, derived from it) also
            // gets the live canvas-background contrast color, since the app UI's
            // own dark/light mode is unrelated to the canvas's independently
            // configurable background.
            const mafsView = container.querySelector<HTMLElement>(".MafsView");
            applyMafsCaptureStyles(mafsView, state.backgroundColor);

            // Mafs renders its axis numbers in a custom web font (CMU Serif);
            // without waiting for it to finish loading, html-to-image can
            // rasterize before the glyphs are painted, leaving them blank.
            await document.fonts?.ready;
            await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

            const dataUrl = await htmlToImage.toPng(container, {
                filter: (node) => node.tagName !== 'I'
            });
            return dataUrl;
        } catch (error) {
            console.error('Failed to generate PNG data URL', error);
            throw new Error('Failed to generate PNG data URL');
        }
    };

    const renderMafsElement = () => {
        try {
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
        } catch (error) {
            console.error('Failed to render Mafs element:', error);
            setErrorMessage('An error occurred while rendering the Mafs element. Please try again.');
            setIsErrorModalOpen(true);
        }
    };

    return (
        <div className="fixed inset-0 z-50 overflow-auto bg-black/50 flex items-start justify-center pt-16">
            <div className={`${className} rounded-lg shadow-lg p-6 w-full max-w-4xl max-h-[calc(100vh-8rem)] overflow-auto`}>
                <div className="flex justify-between items-center mb-4">
                    <h2 className="text-2xl font-bold text-black dark:text-white">{mafsElement.name}</h2>
                    <button
                        className="text-gray-500 hover:text-gray-700 dark:hover:text-white"
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
                    <label htmlFor="width" className="block font-medium mb-1 text-black dark:text-white">
                        Width
                    </label>
                    <input
                        type="number"
                        id="width"
                        max={835}
                        maxLength={3}
                        className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
                        value={mafsConfig.width}
                        onChange={(e) =>
                            handleMafsConfigChange("width", parseInt(e.target.value))
                        }
                    />
                </div>
                <div className="mb-4">
                    <label htmlFor="height" className="block font-medium mb-1 text-black dark:text-white">
                        Height
                    </label>
                    <input
                        type="number"
                        id="height"
                        max={235}
                        maxLength={3}
                        className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
                        <span className="ml-2 text-black dark:text-white">Pan</span>
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
                        <span className="ml-2 text-black dark:text-white">Zoom</span>
                    </label>
                </div>
                <div className="mb-4">
                    <label htmlFor="coordinateType" className="block font-medium mb-1 text-black dark:text-white">
                        Coordinate Type
                    </label>
                    <select
                        id="coordinateType"
                        className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
                <div className="border border-gray-300 rounded-sm p-4 flex justify-center">
                    <div ref={mafsRef}>
                        <Mafs
                            width={mafsConfig.width}
                            height={mafsConfig.height}
                            pan={mafsConfig.pan}
                            zoom={mafsConfig.zoom}
                        >
                            {mafsConfig.coordinateType === "cartesian" && <Coordinates.Cartesian />}
                            {mafsConfig.coordinateType === "polar" && <Coordinates.Polar />}
                            {renderMafsElement()}
                        </Mafs>
                    </div>
                </div>
                <div className="mt-4 flex justify-end">
                    <button className="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded-sm mr-2" onClick={handleAddResource} >
                        Add Element
                    </button>
                </div>
                <Modal
                    isOpen={isErrorModalOpen}
                    onRequestClose={() => setIsErrorModalOpen(false)}
                    contentLabel="Error Modal"
                    className="fixed inset-0 flex items-center justify-center z-9999 bg-black/50"
                    overlayClassName="fixed inset-0 z-9999 bg-black/50"
                    shouldCloseOnOverlayClick={true}
                >
                    <div className="bg-white dark:bg-gray-800 p-6 rounded-sm shadow-lg max-w-md mx-auto">
                        <div className="flex justify-between items-center mb-4">
                            <h2 className="text-2xl font-bold text-red-800 dark:text-red-400">Error</h2>
                        </div>
                        <p className="text-gray-700 dark:text-gray-300 mb-4">{errorMessage}</p>
                        <div className="flex justify-end">
                            <button
                                onClick={() => setIsErrorModalOpen(false)}
                                className="bg-red-500 hover:bg-red-600 text-white font-bold py-2 px-4 rounded-sm focus:outline-hidden focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
                            >
                                Close
                            </button>
                        </div>
                    </div>
                </Modal>
            </div>
        </div >
    );
});

export default dynamic(() => Promise.resolve(MafsModal), { ssr: false });
