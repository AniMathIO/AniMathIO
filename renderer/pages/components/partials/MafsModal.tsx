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
                return <Polygon points={polygonPoints} />;
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
                    </select>
                </div>
                {mafsElement.name === "Line" && (
                    <>
                        <div className="mb-4">
                            <label htmlFor="lineX1" className="block font-medium mb-1">
                                X1
                            </label>
                            <input
                                type="number"
                                id="lineX1"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={lineProps.point1[0]}
                                onChange={(e) =>
                                    setLineProps({
                                        ...lineProps,
                                        point1: [parseFloat(e.target.value), lineProps.point1[1]],
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="lineY1" className="block font-medium mb-1">
                                Y1
                            </label>
                            <input
                                type="number"
                                id="lineY1"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={lineProps.point1[1]}
                                onChange={(e) =>
                                    setLineProps({
                                        ...lineProps,
                                        point1: [lineProps.point1[0], parseFloat(e.target.value)],
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="lineX2" className="block font-medium mb-1">
                                X2
                            </label>
                            <input
                                type="number"
                                id="lineX2"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={lineProps.point2[0]}
                                onChange={(e) =>
                                    setLineProps({
                                        ...lineProps,
                                        point2: [parseFloat(e.target.value), lineProps.point2[1]],
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="lineY2" className="block font-medium mb-1">
                                Y2
                            </label>
                            <input
                                type="number"
                                id="lineY2"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={lineProps.point2[1]}
                                onChange={(e) =>
                                    setLineProps({
                                        ...lineProps,
                                        point2: [lineProps.point2[0], parseFloat(e.target.value)],
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
                            <label htmlFor="circleCenterX" className="block font-medium mb-1">
                                Center X
                            </label>
                            <input
                                type="number"
                                id="circleCenterX"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={circleProps.center[0]}
                                onChange={(e) =>
                                    setCircleProps({
                                        ...circleProps,
                                        center: [parseFloat(e.target.value), circleProps.center[1]],
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="circleCenterY" className="block font-medium mb-1">
                                Center Y
                            </label>
                            <input
                                type="number"
                                id="circleCenterY"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={circleProps.center[1]}
                                onChange={(e) =>
                                    setCircleProps({
                                        ...circleProps,
                                        center: [circleProps.center[0], parseFloat(e.target.value)],
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
                    <>
                        {polygonPoints.map((point, index) => (
                            <div key={index} className="mb-4">
                                <label htmlFor={`polygonX${index}`} className="block font-medium mb-1">
                                    X{index + 1}
                                </label>
                                <input
                                    type="number"
                                    id={`polygonX${index}`}
                                    className="w-full border border-gray-300 rounded px-3 py-2"
                                    value={point[0]}
                                    onChange={(e) => {
                                        const newPoints = [...polygonPoints];
                                        newPoints[index][0] = parseFloat(e.target.value);
                                        setPolygonPoints(newPoints);
                                    }}
                                />
                                <label htmlFor={`polygonY${index}`} className="block font-medium mb-1">
                                    Y{index + 1}
                                </label>
                                <input
                                    type="number"
                                    id={`polygonY${index}`}
                                    className="w-full border border-gray-300 rounded px-3 py-2"
                                    value={point[1]}
                                    onChange={(e) => {
                                        const newPoints = [...polygonPoints];
                                        newPoints[index][1] = parseFloat(e.target.value);
                                        setPolygonPoints(newPoints);
                                    }}
                                />
                            </div>
                        ))}
                        <button
                            className="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded"
                            onClick={() =>
                                setPolygonPoints([...polygonPoints, [0, 0]])
                            }
                        >
                            Add Point
                        </button>
                    </>
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
                            <label htmlFor="latexAtX" className="block font-medium mb-1">
                                At X
                            </label>
                            <input
                                type="number"
                                id="latexAtX"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={latexProps.at[0]}
                                onChange={(e) =>
                                    setLatexProps({
                                        ...latexProps,
                                        at: [parseFloat(e.target.value), latexProps.at[1]],
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="latexAtY" className="block font-medium mb-1">
                                At Y
                            </label>
                            <input
                                type="number"
                                id="latexAtY"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={latexProps.at[1]}
                                onChange={(e) =>
                                    setLatexProps({
                                        ...latexProps,
                                        at: [latexProps.at[0], parseFloat(e.target.value)],
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
                            <label htmlFor="ellipseCenterX" className="block font-medium mb-1">
                                Center X
                            </label>
                            <input
                                type="number"
                                id="ellipseCenterX"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={ellipseProps.center[0]}
                                onChange={(e) =>
                                    setEllipseProps({
                                        ...ellipseProps,
                                        center: [parseFloat(e.target.value), ellipseProps.center[1]],
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="ellipseCenterY" className="block font-medium mb-1">
                                Center Y
                            </label>
                            <input
                                type="number"
                                id="ellipseCenterY"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={ellipseProps.center[1]}
                                onChange={(e) =>
                                    setEllipseProps({
                                        ...ellipseProps,
                                        center: [ellipseProps.center[0], parseFloat(e.target.value)],
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="ellipseRadiusX" className="block font-medium mb-1">
                                Radius X
                            </label>
                            <input
                                type="number"
                                id="ellipseRadiusX"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={ellipseProps.radius[0]}
                                onChange={(e) =>
                                    setEllipseProps({
                                        ...ellipseProps,
                                        radius: [parseFloat(e.target.value), ellipseProps.radius[1]],
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="ellipseRadiusY" className="block font-medium mb-1">
                                Radius Y
                            </label>
                            <input
                                type="number"
                                id="ellipseRadiusY"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={ellipseProps.radius[1]}
                                onChange={(e) =>
                                    setEllipseProps({
                                        ...ellipseProps,
                                        radius: [ellipseProps.radius[0], parseFloat(e.target.value)],
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
                            <label htmlFor="vectorTailX" className="block font-medium mb-1">
                                Tail X
                            </label>
                            <input
                                type="number"
                                id="vectorTailX"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={vectorProps?.tail[0]}
                                onChange={(e) =>
                                    setVectorProps({
                                        ...vectorProps,
                                        tail: [parseFloat(e.target.value), vectorProps?.tail[1]],
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="vectorTailY" className="block font-medium mb-1">
                                Tail Y
                            </label>
                            <input
                                type="number"
                                id="vectorTailY"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={vectorProps.tail[1]}
                                onChange={(e) =>
                                    setVectorProps({
                                        ...vectorProps,
                                        tail: [vectorProps?.tail[0], parseFloat(e.target.value)],
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="vectorTipX" className="block font-medium mb-1">
                                Tip X
                            </label>
                            <input
                                type="number"
                                id="vectorTipX"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={vectorProps.tip[0]}
                                onChange={(e) =>
                                    setVectorProps({
                                        ...vectorProps,
                                        tip: [parseFloat(e.target.value), vectorProps.tip[1]],
                                    })
                                }
                            />
                        </div>
                        <div className="mb-4">
                            <label htmlFor="vectorTipY" className="block font-medium mb-1">
                                Tip Y
                            </label>
                            <input
                                type="number"
                                id="vectorTipY"
                                className="w-full border border-gray-300 rounded px-3 py-2"
                                value={vectorProps.tip[1]}
                                onChange={(e) =>
                                    setVectorProps({
                                        ...vectorProps,
                                        tip: [vectorProps.tip[0], parseFloat(e.target.value)],
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
        </div >
    );
});

export default MafsModal;