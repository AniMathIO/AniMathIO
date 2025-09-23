import { TextProps } from "mafs";
import dynamic from "next/dynamic";

type TextInputsProps = {
    textProps: TextProps;
    setTextProps: (textProps: TextProps) => void;
}
const TextInputs = ({ textProps, setTextProps }: TextInputsProps) => {
    return (
        <>
            <div className="mb-4">
                <label htmlFor="textX" className="block font-medium mb-1 text-black dark:text-white">
                    X
                </label>
                <input
                    type="number"
                    id="textX"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
                <label htmlFor="textY" className="block font-medium mb-1 text-black dark:text-white">
                    Y
                </label>
                <input
                    type="number"
                    id="textY"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
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
                <label htmlFor="textContent" className="block font-medium mb-1 text-black dark:text-white">
                    Content
                </label>
                <input
                    type="text"
                    id="textContent"
                    className="w-full border border-gray-300 dark:border-gray-600 text-black dark:text-white bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
                    value={textProps?.children?.toString() || ""}
                    onChange={(e) =>
                        setTextProps({
                            ...textProps,
                            children: e.target.value,
                        })
                    }
                />
            </div>
        </>
    );
}

export default dynamic(() => Promise.resolve(TextInputs), { ssr: false });