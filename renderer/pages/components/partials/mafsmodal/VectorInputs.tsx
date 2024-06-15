import { VectorProps } from "mafs";
import dynamic from "next/dynamic";

type VectorInputsProps = {
    vectorProps: VectorProps;
    setVectorProps: (vectorProps: VectorProps) => void;
}
const VectorInputs = ({ vectorProps, setVectorProps }: VectorInputsProps) => {
    return (
        <>
            <div className="mb-4">
                <label htmlFor="vectorTailX" className="block font-medium mb-1">
                    Tail X
                </label>
                <input
                    type="number"
                    id="vectorTailX"
                    className="w-full border text-black border-gray-300 rounded px-3 py-2"
                    value={vectorProps ? vectorProps.tail![0] : 0}
                    onChange={(e) =>
                        setVectorProps({
                            ...vectorProps,
                            tail: [parseFloat(e.target.value), vectorProps?.tail![1]],
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
                    className="w-full border text-black border-gray-300 rounded px-3 py-2"
                    value={vectorProps ? vectorProps.tail![1] : 0}
                    onChange={(e) =>
                        setVectorProps({
                            ...vectorProps,
                            tail: [vectorProps?.tail![0], parseFloat(e.target.value)],
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
                    className="w-full border text-black border-gray-300 rounded px-3 py-2"
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
                    className="w-full border text-black border-gray-300 rounded px-3 py-2"
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
    );
}
export default dynamic(() => Promise.resolve(VectorInputs), { ssr: false });