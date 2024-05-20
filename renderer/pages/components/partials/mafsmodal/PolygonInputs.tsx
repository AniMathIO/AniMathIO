import { vec } from "mafs";
import dynamic from "next/dynamic";

type PolygonInputsProps = {
    polygonPoints: vec.Vector2[];
    setPolygonPoints: React.Dispatch<React.SetStateAction<vec.Vector2[]>>;
}
const PolygonInputs = ({ polygonPoints, setPolygonPoints }: PolygonInputsProps) => {
    return (
        <>
            {polygonPoints.map((point, index) => (
                <div key={index} className="mb-4">
                    <label htmlFor={`polygonX${index}`} className="block font-medium mb-1">
                        X{index + 1}
                    </label>
                    <input
                        type="number"
                        id={`polygonX${index}`}
                        className="w-full border border-gray-300 rounded px-3 py-2"
                        value={point[0]}
                        onChange={(e) => {
                            const newPoints = [...polygonPoints];
                            newPoints[index][0] = parseFloat(e.target.value);
                            setPolygonPoints(newPoints);
                        }}
                    />
                    <label htmlFor={`polygonY${index}`} className="block font-medium mb-1">
                        Y{index + 1}
                    </label>
                    <input
                        type="number"
                        id={`polygonY${index}`}
                        className="w-full border border-gray-300 rounded px-3 py-2"
                        value={point[1]}
                        onChange={(e) => {
                            const newPoints = [...polygonPoints];
                            newPoints[index][1] = parseFloat(e.target.value);
                            setPolygonPoints(newPoints);
                        }}
                    />
                </div>
            ))}
            <button
                className="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded mb-2"
                onClick={() =>
                    setPolygonPoints([...polygonPoints, [0, 0]])
                }
            >
                Add Point
            </button>
        </>
    );
}
export default dynamic(() => Promise.resolve(PolygonInputs), { ssr: false });