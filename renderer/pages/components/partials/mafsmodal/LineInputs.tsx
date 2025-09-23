import { SegmentProps } from "mafs"
import dynamic from "next/dynamic";
type LineInputsProps = {
    lineProps: SegmentProps;
    setLineProps: (props: SegmentProps) => void;
};

const LineInputs = ({ lineProps, setLineProps }: LineInputsProps) => {
    return (
        <>
            <div className="mb-4">
                <label htmlFor="lineX1" className="block font-medium mb-1 text-black dark:text-white">
                    X1
                </label>
                <input
                    type="number"
                    id="lineX1"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
                <label htmlFor="lineY1" className="block font-medium mb-1 text-black dark:text-white">
                    Y1
                </label>
                <input
                    type="number"
                    id="lineY1"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
                <label htmlFor="lineX2" className="block font-medium mb-1 text-black dark:text-white">
                    X2
                </label>
                <input
                    type="number"
                    id="lineX2"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
                <label htmlFor="lineY2" className="block font-medium mb-1 text-black dark:text-white">
                    Y2
                </label>
                <input
                    type="number"
                    id="lineY2"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
    );
}
export default dynamic(() => Promise.resolve(LineInputs), { ssr: false });