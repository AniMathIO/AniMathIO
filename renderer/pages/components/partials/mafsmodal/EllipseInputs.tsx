import { EllipseProps } from "mafs";
import dynamic from "next/dynamic";

type EllipseInputsProps = {
    ellipseProps: EllipseProps;
    setEllipseProps: (props: EllipseProps) => void;
};

const EllipseInputs = ({ ellipseProps, setEllipseProps }: EllipseInputsProps) => {
    return (
        <>
            <div className="mb-4">
                <label htmlFor="ellipseCenterX" className="block font-medium mb-1 text-black dark:text-white">
                    Center X
                </label>
                <input
                    type="number"
                    id="ellipseCenterX"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
                <label htmlFor="ellipseCenterY" className="block font-medium mb-1 text-black dark:text-white">
                    Center Y
                </label>
                <input
                    type="number"
                    id="ellipseCenterY"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
                <label htmlFor="ellipseRadiusX" className="block font-medium mb-1 text-black dark:text-white">
                    Radius X
                </label>
                <input
                    type="number"
                    id="ellipseRadiusX"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
                <label htmlFor="ellipseRadiusY" className="block font-medium mb-1 text-black dark:text-white">
                    Radius Y
                </label>
                <input
                    type="number"
                    id="ellipseRadiusY"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
    );
}

export default dynamic(() => Promise.resolve(EllipseInputs), { ssr: false });