import React, { useState, useRef, useContext } from "react";
import { observer } from "mobx-react";
import {
    Mafs,
    Coordinates, Line,
    Point, Circle, Polygon, Text,
    LaTeX, Ellipse, Plot, Vector, Transform,
    PointProps, CircleProps, PolygonProps,
    TextProps, SegmentProps, EllipseProps,
    VectorProps, TransformProps, vec,
} from "mafs";
import { StateContext } from "@/states";
import * as htmlToImage from 'html-to-image';
import { MafsModalProps } from "@/types";
import { KatexOptions } from "katex";
declare interface LatexProps {
    tex: string;
    at: vec.Vector2;
    color?: string;
    katexOptions?: KatexOptions;
}
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

    const [pointProps, setPointProps] = useState<PointProps>({
        x: 0,
        y: 2.3,
    });

    const [circleProps, setCircleProps] = useState<CircleProps>({
        center: [0, 2.3],
        radius: 1,
    });

    const [polygonProps, setPolygonProps] = useState<PolygonProps>({
        points: [[-1, 2], [1, 1.5], [1, 3]],
    });

    const [textProps, setTextProps] = useState<TextProps>({
        x: 0,
        y: 2.3,
        children: "Hello, AniMathIO!",
    });

    const [latexProps, setLatexProps] = useState<LatexProps>({
        at: [0, 2.3],
        tex: String.raw`\begin{bmatrix} ${[1, 0]} \\ ${[0, 1]} \end{bmatrix}`,
    });

    const [ellipseProps, setEllipseProps] = useState<EllipseProps>({
        center: [0, 2.3],
        radius: [2, 1],
    });

    const [plotProps, setPlotProps] = useState({
        y: (x: number) => Math.exp(x),
    });

    const [vectorProps, setVectorProps] = useState<VectorProps>({
        tail: [-1, 1],
        tip: [2, 3],
    });

    const [transformProps, setTransformProps] = useState<TransformProps>({
        scale: [1, 1],
        translate: [0, 0],
        rotate: 0,
    });

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
        switch (mafsElement.name) {
            case "Line":
                return <Line.Segment {...lineProps} />;
            case "Point":
                return <Point {...pointProps} />;
            case "Circle":
                return <Circle {...circleProps} />;
            case "Polygon":
                return <Polygon {...polygonProps} />;
            case "Text":
                return <Text {...textProps} />;
            case "LaTeX":
                return <LaTeX {...latexProps} />;
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
        <div className="fixed h-full p-5 inset-0 z-50 overflow-auto bg-black bg-opacity-50 flex items-center justify-center">
            <div className=" bg-white rounded-lg shadow-lg p-6 w-full max-w-4xl">
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
                    </select>
                </div>
                {mafsElement.name === "Line" && (
                    <>
                        <div className="mb-4">
                            <label htmlFor="linePoint1" className="block font-medium mb-1">
                                Point 1
                            </label>
                            <input
                                type="text"
                                id="linePoint1"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={lineProps.point1.join(",")}
                                onChange={(e) =>
                                    setLineProps({
                                        ...lineProps,
                                        point1: e.target.value.split(",").map(parseFloat),
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="linePoint2" className="block font-medium mb-1">
                                Point 2
                            </label>
                            <input
                                type="text"
                                id="linePoint2"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={lineProps.point2.join(",")}
                                onChange={(e) =>
                                    setLineProps({
                                        ...lineProps,
                                        point2: e.target.value.split(",").map(parseFloat),
                                    })
                                }
                            />
                        </div>
                    </>
                )}
                {mafsElement.name === "Point" && (
                    <>
                        <div className="mb-4">
                            <label htmlFor="pointX" className="block font-medium mb-1">
                                X
                            </label>
                            <input
                                type="number"
                                id="pointX"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={pointProps.x}
                                onChange={(e) =>
                                    setPointProps({
                                        ...pointProps,
                                        x: parseFloat(e.target.value),
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="pointY" className="block font-medium mb-1">
                                Y
                            </label>
                            <input
                                type="number"
                                id="pointY"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={pointProps.y}
                                onChange={(e) =>
                                    setPointProps({
                                        ...pointProps,
                                        y: parseFloat(e.target.value),
                                    })
                                }
                            />
                        </div>
                    </>
                )}
                {mafsElement.name === "Circle" && (
                    <>
                        <div className="mb-4">
                            <label htmlFor="circleCenter" className="block font-medium mb-1">
                                Center
                            </label>
                            <input
                                type="text"
                                id="circleCenter"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={circleProps.center.join(",")}
                                onChange={(e) =>
                                    setCircleProps({
                                        ...circleProps,
                                        center: e.target.value.split(",").map(parseFloat),
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="circleRadius" className="block font-medium mb-1">
                                Radius
                            </label>
                            <input
                                type="number"
                                id="circleRadius"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={circleProps.radius}
                                onChange={(e) =>
                                    setCircleProps({
                                        ...circleProps,
                                        radius: parseFloat(e.target.value),
                                    })
                                }
                            />
                        </div>
                    </>
                )}
                {mafsElement.name === "Polygon" && (
                    <div className="mb-4">
                        <label htmlFor="polygonPoints" className="block font-medium mb-1">
                            Points
                        </label>
                        <textarea
                            id="polygonPoints"
                            className="w-full border border-gray-300 rounded px-3 py-2"
                            value={polygonProps.points
                                .map((point) => point.join(","))
                                .join("\n")}
                            onChange={(e) =>
                                setPolygonProps({
                                    ...polygonProps,
                                    points: e.target.value
                                        .split("\n")
                                        .map((line) => line.split(",").map(parseFloat)),
                                })
                            }
                        />
                    </div>
                )}
                {mafsElement.name === "Text" && (
                    <>
                        <div className="mb-4">
                            <label htmlFor="textX" className="block font-medium mb-1">
                                X
                            </label>
                            <input
                                type="number"
                                id="textX"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={textProps.x}
                                onChange={(e) =>
                                    setTextProps({
                                        ...textProps,
                                        x: parseFloat(e.target.value),
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="textY" className="block font-medium mb-1">
                                Y
                            </label>
                            <input
                                type="number"
                                id="textY"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={textProps.y}
                                onChange={(e) =>
                                    setTextProps({
                                        ...textProps,
                                        y: parseFloat(e.target.value),
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="textContent" className="block font-medium mb-1">
                                Content
                            </label>
                            <input
                                type="text"
                                id="textContent"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={textProps.children}
                                onChange={(e) =>
                                    setTextProps({
                                        ...textProps,
                                        children: e.target.value,
                                    })
                                }
                            />
                        </div>
                    </>
                )}
                {mafsElement.name === "LaTeX" && (
                    <>
                        <div className="mb-4">
                            <label htmlFor="latexAt" className="block font-medium mb-1">
                                At
                            </label>
                            <input
                                type="text"
                                id="latexAt"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={latexProps.at.join(",")}
                                onChange={(e) =>
                                    setLatexProps({
                                        ...latexProps,
                                        at: e.target.value.split(",").map(parseFloat),
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="latexTex" className="block font-medium mb-1">
                                TeX
                            </label>
                            <textarea
                                id="latexTex"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={latexProps.tex}
                                onChange={(e) =>
                                    setLatexProps({
                                        ...latexProps,
                                        tex: e.target.value,
                                    })
                                }
                            />
                        </div>
                    </>
                )}
                {mafsElement.name === "Ellipse" && (
                    <>
                        <div className="mb-4">
                            <label htmlFor="ellipseCenter" className="block font-medium mb-1">
                                Center
                            </label>
                            <input
                                type="text"
                                id="ellipseCenter"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={ellipseProps.center.join(",")}
                                onChange={(e) =>
                                    setEllipseProps({
                                        ...ellipseProps,
                                        center: e.target.value.split(",").map(parseFloat),
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="ellipseRadius" className="block font-medium mb-1">
                                Radius
                            </label>
                            <input
                                type="text"
                                id="ellipseRadius"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={ellipseProps.radius.join(",")}
                                onChange={(e) =>
                                    setEllipseProps({
                                        ...ellipseProps,
                                        radius: e.target.value.split(",").map(parseFloat),
                                    })
                                }
                            />
                        </div>
                    </>
                )}
                {mafsElement.name === "Plot" && (
                    <div className="mb-4">
                        <label htmlFor="plotFunction" className="block font-medium mb-1">
                            Function
                        </label>
                        <input
                            type="text"
                            id="plotFunction"
                            className="w-full border border-gray-300 rounded px-3 py-2"
                            value={plotProps.y.toString()}
                            onChange={(e) =>
                                setPlotProps({
                                    ...plotProps,
                                    y: eval(e.target.value),
                                })
                            }
                        />
                    </div>
                )}
                {mafsElement.name === "Vector" && (
                    <>
                        <div className="mb-4">
                            <label htmlFor="vectorTail" className="block font-medium mb-1">
                                Tail
                            </label>
                            <input
                                type="text"
                                id="vectorTail"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={vectorProps.tail.join(",")}
                                onChange={(e) =>
                                    setVectorProps({
                                        ...vectorProps,
                                        tail: e.target.value.split(",").map(parseFloat),
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="vectorTip" className="block font-medium mb-1">
                                Tip
                            </label>
                            <input
                                type="text"
                                id="vectorTip"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={vectorProps.tip.join(",")}
                                onChange={(e) =>
                                    setVectorProps({
                                        ...vectorProps,
                                        tip: e.target.value.split(",").map(parseFloat),
                                    })
                                }
                            />
                        </div>
                    </>
                )}
                <div className="border border-gray-300 rounded p-4 flex justify-center">
                    <div ref={mafsRef}>
                        <Mafs width={mafsConfig.width} height={mafsConfig.height} pan={mafsConfig.pan} zoom={mafsConfig.zoom} >
                            {mafsConfig.coordinateType === "cartesian" ? (
                                <Coordinates.Cartesian />
                            ) : (
                                <Coordinates.Polar />
                            )}
                            <Transform scale={transformProps.scale} translate={transformProps.translate} rotate={transformProps.rotate} >
                                {renderMafsElement()}
                            </Transform>
                        </Mafs>
                    </div>
                </div>
                <div className="mt-4 flex justify-end">
                    <button className="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded mr-2" onClick={handleAddResource} >
                        Add Element
                    </button>
                </div>
            </div>
        </div>
    );
});

export default MafsModal;