import { TextProps } from "mafs";

type TextInputsProps = {
    textProps: TextProps;
    setTextProps: (textProps: TextProps) => void;
}
const TextInputs = ({ textProps, setTextProps }: TextInputsProps) => {
    return (
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

export default TextInputs;