type PlotInputsProps = {
    plotProps: {
        y: (x: number) => number;
    }
    setPlotProps: (props: { y: (x: number) => number }) => void;
    plotInput: string;
    setPlotInput: (input: string) => void;
    handlePlotSubmit: () => void;
}
const PlotInputs = ({ plotProps, setPlotProps, plotInput, setPlotInput, handlePlotSubmit }: PlotInputsProps) => {
    return (
        <>
            <div className="mb-4">
                <label htmlFor="plotFunction" className="block font-medium mb-1">
                    Function
                </label>
                <input
                    type="text"
                    id="plotFunction"
                    className="w-full border border-gray-300 rounded px-3 py-2"
                    value={plotInput}
                    onChange={(e) => setPlotInput(e.target.value)}
                />
            </div>
            <button className="bg-blue-500 mb-2 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded" onClick={handlePlotSubmit} >
                Submit Plot
            </button>
        </>
    );
}

export default PlotInputs;