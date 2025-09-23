import { PointProps } from "mafs";
import dynamic from "next/dynamic";

type PointInputsProps = {
    pointProps: PointProps;
    setPointProps: (props: PointProps) => void;
}
const PointInputs = ({ pointProps, setPointProps }: PointInputsProps) => {
    return (
        <>
            <div className="mb-4">
                <label htmlFor="pointX" className="block font-medium mb-1 text-black dark:text-white">
                    X
                </label>
                <input
                    type="number"
                    id="pointX"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
                <label htmlFor="pointY" className="block font-medium mb-1 text-black dark:text-white">
                    Y
                </label>
                <input
                    type="number"
                    id="pointY"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
    );
}

export default dynamic(() => Promise.resolve(PointInputs), { ssr: false });