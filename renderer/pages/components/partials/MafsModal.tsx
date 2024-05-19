import React, { useState, useRef, useContext } from "react";
import { observer } from "mobx-react";
import { Mafs, Coordinates } from "mafs";
import { StateContext } from "@/states";
import * as htmlToImage from 'html-to-image';
import { MafsModalProps } from "@/types";

const MafsModal = observer(({ isOpen, onClose, mafsElement }: MafsModalProps) => {
    const [mafsConfig, setMafsConfig] = useState({
        width: 500,
        height: 200,
        pan: true,
        zoom: true,
        coordinateType: mafsElement.coordinateType || "cartesian",
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
                <div className="border border-gray-300 rounded p-4 flex justify-center">
                    <div ref={mafsRef}>
                        <Mafs width={mafsConfig.width} height={mafsConfig.height} pan={mafsConfig.pan} zoom={mafsConfig.zoom} >
                            {mafsConfig.coordinateType === "cartesian" ? (
                                <Coordinates.Cartesian />
                            ) : (
                                <Coordinates.Polar />
                            )}
                            {mafsElement.children}
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