import { CircleProps } from "mafs";
import dynamic from "next/dynamic";

type CircleInputsProps = {
    circleProps: CircleProps;
    setCircleProps: (props: CircleProps) => void;
};

const CircleInputs = ({ circleProps, setCircleProps }: CircleInputsProps) => {
    return (
        <>
            <div className="mb-4">
                <label htmlFor="circleCenterX" className="block font-medium mb-1">
                    Center X
                </label>
                <input
                    type="number"
                    id="circleCenterX"
                    className="w-full border border-gray-300 text-black rounded px-3 py-2"
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
                    className="w-full border border-gray-300 text-black rounded px-3 py-2"
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
                    className="w-full border border-gray-300 text-black rounded px-3 py-2"
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
    )
}

export default dynamic(() => Promise.resolve(CircleInputs), { ssr: false });