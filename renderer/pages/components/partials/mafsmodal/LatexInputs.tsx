import { LatexProps } from "@/types";
import dynamic from "next/dynamic";
import TextToLatex from "./TextToLatex";

type LatexInputsProps = {
    latexProps: LatexProps;
    setLatexProps: (latexProps: LatexProps) => void;
    latexInput: string;
    setLatexInput: (latexInput: string) => void;
    handleLatexSubmit: () => void;
};

const LatexInputs = ({
    latexProps,
    setLatexProps,
    latexInput,
    setLatexInput,
    handleLatexSubmit,
}: LatexInputsProps) => {
    return (
        <>
            <TextToLatex
                latexProps={latexProps}
                setLatexProps={setLatexProps}
                latexInput={latexInput}
                setLatexInput={setLatexInput}
                handleLatexSubmit={handleLatexSubmit}
            />
            <div className="mb-4">
                <label htmlFor="latexAtX" className="block font-medium mb-1">
                    At X
                </label>
                <input
                    type="number"
                    id="latexAtX"
                    className="w-full border text-black border-gray-300 rounded px-3 py-2"
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
                    className="w-full border text-black border-gray-300 rounded px-3 py-2"
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
                    className="w-full border text-black border-gray-300 rounded px-3 py-2"
                    value={latexInput}
                    onChange={(e) => setLatexInput(e.target.value)}
                />
            </div>
            <button
                className="bg-blue-500 mb-2 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded"
                onClick={handleLatexSubmit}
            >
                Submit LaTeX
            </button>
        </>
    );
};

export default dynamic(() => Promise.resolve(LatexInputs), { ssr: false });