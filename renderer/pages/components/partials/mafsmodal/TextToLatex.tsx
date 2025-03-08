import React, { useState, useEffect } from 'react';
import { LatexProps } from "@/types";
import dynamic from 'next/dynamic';

type TextToLatexProps = {
    latexProps: LatexProps;
    setLatexProps: (latexProps: LatexProps) => void;
    latexInput: string;
    setLatexInput: (latexInput: string) => void;
    handleLatexSubmit: () => void;
};

const TextToLatex: React.FC<TextToLatexProps> = ({
    latexProps,
    setLatexProps,
    latexInput,
    setLatexInput,
    handleLatexSubmit,
}) => {
    const [textInput, setTextInput] = useState('');
    const [isConverting, setIsConverting] = useState(false);
    const [hasApiToken, setHasApiToken] = useState(false);
    const [error, setError] = useState('');

    // Check if API token exists when component mounts
    useEffect(() => {
        const checkApiToken = async () => {
            try {
                const token = await window.electron.ipcRenderer.invoke('get-gemini-api-token');
                setHasApiToken(!!token);
            } catch (error) {
                console.error("Failed to check API token:", error);
                setHasApiToken(false);
            }
        };

        checkApiToken();
    }, []);

    const handleConvert = async () => {
        if (!textInput.trim()) {
            setError('Please enter some text to convert');
            return;
        }

        setError('');
        setIsConverting(true);

        try {
            // Get the API token
            const apiToken = await window.electron.ipcRenderer.invoke('get-gemini-api-token');

            if (!apiToken) {
                setError('No API token found. Please add a Gemini API token in Settings.');
                setIsConverting(false);
                return;
            }

            // Call Gemini API to convert text to LaTeX
            const response = await fetch(`https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key=${apiToken}`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    contents: [{
                        parts: [{
                            text: `Convert the following mathematical text to LaTeX. Return only the LaTeX code without any explanations or markdown formatting, DO NOT add '$' symbols around it:\n\n${textInput}`
                        }]
                    }],
                    generationConfig: {
                        temperature: 0.2,
                        topK: 32,
                        topP: 1,
                        maxOutputTokens: 1024,
                    }
                })
            });

            if (!response.ok) {
                throw new Error(`API request failed: ${response.statusText}`);
            }

            const data = await response.json();
            const latexResult = data.candidates[0].content.parts[0].text;

            // Clean the latex result (remove any potential markdown formatting)
            let cleanLatex = latexResult.trim();

            // If surrounded by backticks, remove them
            if (cleanLatex.startsWith('```latex') && cleanLatex.endsWith('```')) {
                cleanLatex = cleanLatex.slice(8, -3).trim();
            } else if (cleanLatex.startsWith('```') && cleanLatex.endsWith('```')) {
                cleanLatex = cleanLatex.slice(3, -3).trim();
            }

            // Update the LaTeX input
            setLatexInput(cleanLatex);

        } catch (error) {
            console.error("Failed to convert text to LaTeX:", error);
            setError('Failed to convert text to LaTeX. Please try again or check your API token.');
        } finally {
            setIsConverting(false);
        }
    };

    if (!hasApiToken) {
        return (
            <div className="bg-yellow-50 border-l-4 border-yellow-400 p-4 mb-4">
                <div className="flex">
                    <div className="ml-3">
                        <p className="text-sm text-yellow-700">
                            To use the text-to-LaTeX conversion, please add a Gemini API token in Settings.
                        </p>
                    </div>
                </div>
            </div>
        );
    }

    return (
        <div className="mb-6 border border-gray-200 dark:border-gray-700 rounded-lg p-4 bg-white dark:bg-gray-800">
            <h3 className="text-lg font-semibold mb-3 dark:text-white">Text to LaTeX Converter ✨</h3>

            <div className="mb-4">
                <label htmlFor="textInput" className="block font-medium mb-1 dark:text-white">
                    Enter mathematical text
                </label>
                <textarea
                    id="textInput"
                    className="w-full border text-black border-gray-300 rounded px-3 py-2 min-h-[100px]"
                    value={textInput}
                    onChange={(e) => setTextInput(e.target.value)}
                    placeholder="Example: The quadratic formula is x = (-b ± √(b² - 4ac)) / 2a"
                />
            </div>

            {error && (
                <div className="mb-4 text-red-500 text-sm">
                    {error}
                </div>
            )}

            <div className="flex justify-between mb-4">
                <button
                    className={`bg-purple-600 hover:bg-purple-700 text-white font-bold py-2 px-4 rounded ${isConverting ? 'opacity-50 cursor-not-allowed' : ''
                        }`}
                    onClick={handleConvert}
                    disabled={isConverting}
                >
                    {isConverting ? 'Converting...' : 'Convert to LaTeX'}
                </button>
            </div>


            <p className=''>The LaTeX code will be displayed below in the LaTeX input field</p>
        </div>
    );
};

export default dynamic(() => Promise.resolve(TextToLatex), { ssr: false });
