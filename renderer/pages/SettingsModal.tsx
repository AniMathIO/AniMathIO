import React, { useState, useEffect } from 'react';
import { SunIcon, MoonIcon } from '@heroicons/react/24/solid';
import { FaArrowLeft, FaArrowRight, FaArrowUp, FaArrowDown, FaTrashAlt, FaCopy, FaPaste, FaEyeSlash, FaEye, FaPlay } from 'react-icons/fa';
import packageJson from '../../package.json';
declare global {
    interface Window {
        electron: {
            ipcRenderer: {
                send(channel: string, data: any): void;
                on(channel: string, func: (...args: any[]) => void): () => void;
                removeListener(channel: string, func: (...args: any[]) => void): void;
                invoke(channel: string, data?: any): Promise<any>;
            };
            requestMicrophonePermission: () => Promise<string>;
            readProjectFile: (filePath: string) => Promise<{
                success: boolean;
                data?: number[];
                fileName?: string;
                filePath?: string;
                error?: string;
            }>;
            openProjectFile: () => Promise<{
                success: boolean;
                data?: number[];
                fileName?: string;
                filePath?: string;
                error?: string;
            }>;
            saveProjectFile: (fileData: number[], suggestedName?: string) => Promise<{
                success: boolean;
                fileName?: string;
                filePath?: string;
                error?: string;
            }>;
            writeProjectFile: (filePath: string, fileData: number[]) => Promise<{
                success: boolean;
                error?: string;
            }>;
            readMediaFile: (filePath: string) => Promise<{
                success: boolean;
                type?: "image" | "video" | "audio";
                dataUrl?: string;
                error?: string;
            }>;
            writeVideoFile: (filePath: string, fileData: number[]) => Promise<{
                success: boolean;
                error?: string;
            }>;
            onOpenFileFromSystem: (callback: (data: {
                success: boolean;
                data?: number[];
                fileName?: string;
                filePath?: string;
                error?: string;
            }) => void) => () => void;
            openExternalUrl: (url: string) => Promise<{
                success: boolean;
                error?: string;
            }>;
        };
    }
}

const SettingsModal: React.FC = () => {
    const [isOpen, setIsOpen] = useState(false);
    const [isDarkMode, setIsDarkMode] = useState(false);
    const [geminiApiToken, setGeminiApiToken] = useState("");
    const [selectedModel, setSelectedModel] = useState("gemini-2.0-flash");
    const [availableModels, setAvailableModels] = useState<string[]>([]);
    const [isLoadingModels, setIsLoadingModels] = useState(false);
    const [isSaving, setIsSaving] = useState(false);
    const [isClearing, setIsClearing] = useState(false);
    const [saveMessage, setSaveMessage] = useState({ text: "", type: "" });
    const [showFullToken, setShowFullToken] = useState(false);
    const [isLoading, setIsLoading] = useState(true);
    const [mcpEnabled, setMcpEnabled] = useState(false);
    const [mcpPort, setMcpPort] = useState<number | null>(null);
    const [mcpToken, setMcpToken] = useState("");
    const [mcpRunning, setMcpRunning] = useState(false);
    const [showFullMcpToken, setShowFullMcpToken] = useState(false);
    const [isLoadingMcp, setIsLoadingMcp] = useState(true);
    const [isSavingMcp, setIsSavingMcp] = useState(false);
    const [isRegeneratingMcpToken, setIsRegeneratingMcpToken] = useState(false);
    const [copiedMcpField, setCopiedMcpField] = useState("");


    useEffect(() => {
        // Sync isDarkMode state with the theme already applied by _app.tsx
        // The theme is already loaded and applied in _app.tsx, so we just need to sync the toggle state
        const isDark = document.documentElement.classList.contains('dark');
        setIsDarkMode(isDark);
    }, []);

    useEffect(() => {
        const loadSettings = async () => {
            if (isOpen) {
                setIsLoading(true);
                try {
                    // Load API token
                    const savedApiToken = await window.electron.ipcRenderer.invoke('get-gemini-api-token');
                    if (savedApiToken) {
                        setGeminiApiToken(savedApiToken);
                    } else {
                        setGeminiApiToken('');
                    }

                    // Load selected model
                    const savedModel = await window.electron.ipcRenderer.invoke('get-gemini-model');
                    if (savedModel) {
                        setSelectedModel(savedModel);
                    } else {
                        setSelectedModel('gemini-2.0-flash');
                    }
                } catch (error) {
                    console.error("Failed to load settings:", error);
                } finally {
                    setIsLoading(false);
                }
            }
        };

        loadSettings();
    }, [isOpen]); // Re-run when isOpen changes

    useEffect(() => {
        if (!isOpen) {
            return;
        }

        let cancelled = false;
        const loadMcpSettings = async () => {
            setIsLoadingMcp(true);
            setMcpEnabled(false);
            setMcpRunning(false);
            setMcpPort(null);
            setMcpToken('');
            setShowFullMcpToken(false);
            try {
                const settings = await window.electron.ipcRenderer.invoke('get-mcp-settings');
                if (!cancelled) {
                    setMcpEnabled(settings.enabled);
                    setMcpPort(settings.port);
                    setMcpToken(settings.token);
                    setMcpRunning(settings.running);
                }
            } catch (error) {
                console.error("Failed to load MCP settings:", error);
                if (!cancelled) {
                    setSaveMessage({ text: "MCP settings are currently unavailable", type: "error" });
                }
            } finally {
                if (!cancelled) {
                    setIsLoadingMcp(false);
                }
            }
        };

        loadMcpSettings();
        return () => {
            cancelled = true;
        };
    }, [isOpen]);

    useEffect(() => {
        if (!copiedMcpField) {
            return;
        }
        const timeout = window.setTimeout(() => setCopiedMcpField(''), 2000);
        return () => window.clearTimeout(timeout);
    }, [copiedMcpField]);

    // Fetch available models when API token is available
    useEffect(() => {
        const fetchModels = async () => {
            if (!geminiApiToken || !isOpen) {
                setAvailableModels([]);
                return;
            }

            setIsLoadingModels(true);
            try {
                // Dynamically import GoogleGenAI to avoid SSR issues
                const { GoogleGenAI } = await import('@google/genai');
                const ai = new GoogleGenAI({ apiKey: geminiApiToken });
                const modelsPager = await ai.models.list();
                
                const modelNames: string[] = [];
                for await (const model of modelsPager) {
                    // Extract model name (format: "models/gemini-2.0-flash" -> "gemini-2.0-flash")
                    if (model.name && model.name.startsWith('models/')) {
                        const modelName = model.name.replace('models/', '');
                        // Filter to only include Gemini models
                        if (modelName.startsWith('gemini-')) {
                            modelNames.push(modelName);
                        }
                    }
                }
                
                // Sort models alphabetically
                modelNames.sort();
                setAvailableModels(modelNames);
            } catch (error) {
                console.error("Failed to fetch models:", error);
                setAvailableModels([]);
            } finally {
                setIsLoadingModels(false);
            }
        };

        fetchModels();
    }, [geminiApiToken, isOpen]);

    useEffect(() => {
        const handleOpenModal = () => {
            setIsOpen(true);
        };

        const unsubscribe = window.electron.ipcRenderer.on('open-settings-modal', handleOpenModal);
        // The native OS menu (which fires the IPC event above) doesn't render
        // reliably on every platform, so the in-app Titlebar fallback opens
        // settings via this plain window event instead of a main-process round trip.
        window.addEventListener('open-settings-modal', handleOpenModal);

        return () => {
            unsubscribe();
            window.removeEventListener('open-settings-modal', handleOpenModal);
        };
    }, []);

    const handleCloseModal = () => {
        setIsOpen(false);
        setSaveMessage({ text: "", type: "" });
    };

    const handleToggleDarkMode = () => {
        const newThemeMode = !isDarkMode ? 'dark' : 'light';
        setIsDarkMode(!isDarkMode);
        document.documentElement.classList.toggle('dark');
        // Save the updated theme mode
        window.electron.ipcRenderer.send('set-theme-mode', newThemeMode);
    };

    const handleSaveApiToken = async () => {
        try {
            setIsSaving(true);
            setSaveMessage({ text: "", type: "" });

            // Basic validation
            if (!geminiApiToken.trim()) {
                setSaveMessage({ text: "API token cannot be empty", type: "error" });
                setIsSaving(false);
                return;
            }

            // Save the API token
            await window.electron.ipcRenderer.invoke('set-gemini-api-token', geminiApiToken);
            
            // Save the selected model
            await window.electron.ipcRenderer.invoke('set-gemini-model', selectedModel);

            setSaveMessage({ text: "API token and model saved successfully!", type: "success" });
        } catch (error) {
            console.error("Failed to save API token:", error);
            setSaveMessage({ text: "Failed to save API token", type: "error" });
        } finally {
            setIsSaving(false);
        }
    };

    const handleClearApiToken = async () => {
        try {
            setIsClearing(true);
            setSaveMessage({ text: "", type: "" });

            // Clear the API token
            await window.electron.ipcRenderer.invoke('set-gemini-api-token', '');
            setGeminiApiToken('');

            setSaveMessage({ text: "API token cleared successfully!", type: "success" });
        } catch (error) {
            console.error("Failed to clear API token:", error);
            setSaveMessage({ text: "Failed to clear API token", type: "error" });
        } finally {
            setIsClearing(false);
        }
    };

    const handleModelChange = async (model: string) => {
        setSelectedModel(model);
        try {
            // Save the model immediately when changed
            await window.electron.ipcRenderer.invoke('set-gemini-model', model);
        } catch (error) {
            console.error("Failed to save model:", error);
        }
    };

    const handleToggleMcp = async () => {
        try {
            setIsSavingMcp(true);
            setSaveMessage({ text: "", type: "" });
            const enabled = !mcpEnabled;
            const result = await window.electron.ipcRenderer.invoke('set-mcp-enabled', enabled);
            setMcpRunning(result.running);
            if (result.success) {
                setMcpEnabled(enabled);
            } else {
                setSaveMessage({ text: result.error || "Failed to update MCP settings", type: "error" });
            }
        } catch (error) {
            console.error("Failed to update MCP settings:", error);
            setSaveMessage({ text: "Unable to update MCP settings. Please try again.", type: "error" });
        } finally {
            setIsSavingMcp(false);
        }
    };

    const handleRegenerateMcpToken = async () => {
        if (!window.confirm("Regenerate the MCP token? Existing agent configurations will stop working until you update them with the new token.")) {
            return;
        }
        try {
            setIsRegeneratingMcpToken(true);
            setSaveMessage({ text: "", type: "" });
            const result = await window.electron.ipcRenderer.invoke('regenerate-mcp-token');
            if (result.success) {
                setMcpToken(result.token);
                setShowFullMcpToken(false);
                setCopiedMcpField('');
                setSaveMessage({ text: "MCP token regenerated. Update your agent configurations.", type: "success" });
            } else {
                setSaveMessage({ text: "Failed to regenerate MCP token", type: "error" });
            }
        } catch (error) {
            console.error("Failed to regenerate MCP token:", error);
            setSaveMessage({ text: "Unable to regenerate MCP token. Please try again.", type: "error" });
        } finally {
            setIsRegeneratingMcpToken(false);
        }
    };

    const handleCopyMcp = async (field: string, value: string) => {
        try {
            await navigator.clipboard.writeText(value);
            setCopiedMcpField(field);
        } catch (error) {
            console.error("Failed to copy MCP connection details:", error);
            setSaveMessage({ text: "Unable to copy. Please copy the value manually.", type: "error" });
        }
    };

    if (!isOpen) {
        return null;
    }

    return (
        <div className="fixed inset-0 flex items-center justify-center z-50">
            <div className="absolute inset-0 bg-black opacity-70"></div>
            <div className="flex flex-col relative w-[650px] max-h-[90vh] overflow-y-auto bg-white dark:bg-gray-800 rounded-lg p-6">
                <h2 className="text-2xl font-bold mb-4 dark:text-white">Settings</h2>
                <div className="flex items-center mb-4">
                    <label className="mr-2 dark:text-white">Dark Mode:</label>
                    <button
                        className="bg-gray-200 hover:bg-gray-400 dark:bg-gray-700 dark:hover:bg-gray-500 p-2 rounded-full"
                        onClick={handleToggleDarkMode}
                    >
                        {isDarkMode ? (
                            <SunIcon className="h-6 w-6 text-gray-800 dark:text-white" />
                        ) : (
                            <MoonIcon className="h-6 w-6 text-gray-800 dark:text-white" />
                        )}
                    </button>
                </div>

                <div className="mb-6">
                    <h3 className="text-xl font-semibold mb-3 dark:text-white">AI Integration ✨</h3>
                    <div className="mb-4">
                        <label htmlFor="geminiApiToken" className="block mb-2 dark:text-white">
                            Gemini API Token:
                        </label>
                        <div className="relative flex gap-2">
                            <div className="grow relative">
                                <input
                                    type={showFullToken ? "text" : "password"}
                                    id="geminiApiToken"
                                    value={isLoading ? "Loading..." : geminiApiToken}
                                    onChange={(e) => setGeminiApiToken(e.target.value)}
                                    className="w-full border text-black dark:text-white border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 rounded-sm px-3 py-2 pr-10"
                                    placeholder="Enter your Gemini API token"
                                    disabled={isLoading}
                                />
                                {geminiApiToken && (
                                    <button
                                        type="button"
                                        className="absolute right-2 top-1/2 transform -translate-y-1/2 text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
                                        onClick={() => setShowFullToken(!showFullToken)}
                                        title={showFullToken ? "Hide token" : "Show token"}
                                    >
                                        {showFullToken ? <FaEyeSlash /> : <FaEye />}
                                    </button>
                                )}
                            </div>
                            <button
                                className={`px-4 py-2 rounded-xl ${isSaving || isLoading || isClearing
                                    ? "bg-gray-400 cursor-not-allowed"
                                    : "bg-blue-500 hover:bg-blue-600 text-white"
                                    }`}
                                onClick={handleSaveApiToken}
                                disabled={isSaving || isLoading || isClearing}
                            >
                                {isSaving ? "Saving..." : "Save"}
                            </button>

                            {geminiApiToken && !isLoading && (
                                <button
                                    className={`px-4 py-2 rounded-xl ${isClearing || isLoading || isSaving
                                        ? "bg-gray-400 cursor-not-allowed"
                                        : "bg-red-500 hover:bg-red-600 text-white"
                                        }`}
                                    onClick={handleClearApiToken}
                                    disabled={isClearing || isLoading || isSaving}
                                >
                                    {isClearing ? "Clearing..." : "Clear Token"}
                                </button>
                            )}
                        </div>

                        {saveMessage.text && (
                            <p className={`mt-2 text-sm ${saveMessage.type === "error" ? "text-red-500" : "text-green-500"
                                }`}>
                                {saveMessage.text}
                            </p>
                        )}
                        <p className="mt-2 text-sm text-gray-600 dark:text-gray-400">
                            Your API token is stored locally and is used to access the Gemini AI for text-to-LaTeX conversion.
                            <br />
                            You can get a Gemini API token from the <a href="https://ai.google.dev/gemini-api/docs/api-key" target="_blank" rel="noopener noreferrer" className="text-blue-500 hover:underline">Google AI Studio</a>.
                        </p>
                    </div>
                    <div className="mb-4">
                        <label htmlFor="geminiModel" className="block mb-2 dark:text-white">
                            Gemini Model:
                        </label>
                        <select
                            id="geminiModel"
                            value={selectedModel}
                            onChange={(e) => handleModelChange(e.target.value)}
                            disabled={isLoading || isLoadingModels || !geminiApiToken}
                            className="w-full border text-black dark:text-white border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
                        >
                            {isLoadingModels ? (
                                <option>Loading models...</option>
                            ) : availableModels.length > 0 ? (
                                availableModels.map((model) => (
                                    <option key={model} value={model}>
                                        {model}
                                    </option>
                                ))
                            ) : (
                                <option value={selectedModel}>{selectedModel || "No models available"}</option>
                            )}
                        </select>
                        <p className="mt-2 text-sm text-gray-600 dark:text-gray-400">
                            Select the Gemini model to use for text-to-LaTeX conversion. Available models are fetched automatically when an API token is provided.
                        </p>
                    </div>
                </div>

                <div className="mb-6">
                    <h3 className="text-xl font-semibold mb-3 dark:text-white">AI Agent Integration (MCP)</h3>
                    <div className="flex items-center gap-2 mb-4">
                        <input
                            type="checkbox"
                            id="mcpEnabled"
                            checked={mcpEnabled}
                            onChange={handleToggleMcp}
                            disabled={isLoadingMcp || isSavingMcp || isRegeneratingMcpToken}
                            className="accent-blue-500"
                        />
                        <label htmlFor="mcpEnabled" className="dark:text-white">Enable MCP server</label>
                        <span className="text-sm text-gray-600 dark:text-gray-400">
                            {isLoadingMcp ? "Loading..." : isSavingMcp ? "Updating..." : mcpRunning ? "Running" : "Stopped"}
                        </span>
                    </div>
                    {mcpEnabled && (
                        <>
                            <div className="mb-4">
                                <label htmlFor="mcpEndpoint" className="block mb-2 dark:text-white">Endpoint URL:</label>
                                <div className="flex gap-2">
                                    <input
                                        type="text"
                                        id="mcpEndpoint"
                                        value={mcpPort === null ? '' : `http://127.0.0.1:${mcpPort}/mcp`}
                                        readOnly
                                        className="grow min-w-0 border text-black dark:text-white border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 rounded-sm px-3 py-2"
                                    />
                                    <button
                                        className="px-4 py-2 rounded-xl bg-blue-500 hover:bg-blue-600 text-white disabled:bg-gray-400 disabled:cursor-not-allowed"
                                        onClick={() => handleCopyMcp('endpoint', `http://127.0.0.1:${mcpPort}/mcp`)}
                                        disabled={mcpPort === null}
                                    >
                                        {copiedMcpField === 'endpoint' ? "Copied" : "Copy"}
                                    </button>
                                </div>
                            </div>
                            <div className="mb-4">
                                <label htmlFor="mcpToken" className="block mb-2 dark:text-white">Auth token:</label>
                                <div className="relative flex gap-2">
                                    <div className="grow min-w-0 relative">
                                        <input
                                            type={showFullMcpToken ? "text" : "password"}
                                            id="mcpToken"
                                            value={mcpToken}
                                            readOnly
                                            className="w-full border text-black dark:text-white border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 rounded-sm px-3 py-2 pr-10"
                                        />
                                        {mcpToken && (
                                            <button
                                                type="button"
                                                className="absolute right-2 top-1/2 transform -translate-y-1/2 text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
                                                onClick={() => setShowFullMcpToken(!showFullMcpToken)}
                                                title={showFullMcpToken ? "Hide token" : "Show token"}
                                                aria-label={showFullMcpToken ? "Hide MCP token" : "Show MCP token"}
                                            >
                                                {showFullMcpToken ? <FaEyeSlash /> : <FaEye />}
                                            </button>
                                        )}
                                    </div>
                                    <button
                                        className="px-4 py-2 rounded-xl bg-blue-500 hover:bg-blue-600 text-white disabled:bg-gray-400 disabled:cursor-not-allowed"
                                        onClick={() => handleCopyMcp('token', mcpToken)}
                                        disabled={!mcpToken || isRegeneratingMcpToken}
                                    >
                                        {copiedMcpField === 'token' ? "Copied" : "Copy"}
                                    </button>
                                </div>
                            </div>
                            <button
                                className={`px-4 py-2 rounded-xl ${isRegeneratingMcpToken || isSavingMcp
                                    ? "bg-gray-400 cursor-not-allowed"
                                    : "bg-red-500 hover:bg-red-600 text-white"
                                    }`}
                                onClick={handleRegenerateMcpToken}
                                disabled={isRegeneratingMcpToken || isSavingMcp}
                            >
                                {isRegeneratingMcpToken ? "Regenerating..." : "Regenerate token"}
                            </button>
                        </>
                    )}
                    <p className="mt-2 text-sm text-gray-600 dark:text-gray-400">
                        A connected AI agent can read and modify the open project and write files. The server listens only on localhost. Keep the token private.
                    </p>
                </div>

                <div className='dark:text-white'>
                    <h2 className="mr-2 dark:text-white">Keyboard Shortcuts:</h2>
                    <table className="w-full border-collapse border border-gray-300 dark:border-gray-700 text-sm bg-white dark:bg-gray-800 rounded-lg overflow-hidden">
                        <thead className="bg-gray-100 dark:bg-gray-700">
                            <tr>
                                <th className="border-b dark:border-gray-600 px-4 py-2 text-left font-medium text-gray-700 dark:text-gray-300">
                                    Key
                                </th>
                                <th className="border-b dark:border-gray-600 px-4 py-2 text-left font-medium text-gray-700 dark:text-gray-300">
                                    Action
                                </th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr className="hover:bg-gray-50 dark:hover:bg-gray-600">
                                <td className="px-4 py-2 text-gray-700 dark:text-gray-300 flex items-center gap-2">
                                    <span className="font-bold">Space</span>
                                    <FaPlay className="text-gray-500 dark:text-gray-400" />
                                </td>
                                <td className="px-4 py-2 text-gray-600 dark:text-gray-400">
                                    Starts or pauses the playback
                                </td>
                            </tr>
                            <tr className="hover:bg-gray-50 dark:hover:bg-gray-600">
                                <td className="px-4 py-2 text-gray-700 dark:text-gray-300 flex items-center gap-2">
                                    <span className="font-bold">Arrows (Left or Right)</span>
                                    <FaArrowLeft className="text-gray-500 dark:text-gray-400" />
                                    <FaArrowRight className="text-gray-500 dark:text-gray-400" />
                                </td>
                                <td className="px-4 py-2 text-gray-600 dark:text-gray-400">
                                    Skips 10 seconds backward or forward
                                </td>
                            </tr>
                            <tr className="hover:bg-gray-50 dark:hover:bg-gray-600">
                                <td className="px-4 py-2 text-gray-700 dark:text-gray-300 flex items-center gap-2">
                                    <span className="font-bold">Ctrl + </span>
                                    <FaArrowUp className="text-gray-500 dark:text-gray-400" />
                                    <FaArrowDown className="text-gray-500 dark:text-gray-400" />
                                    <FaArrowLeft className="text-gray-500 dark:text-gray-400" />
                                    <FaArrowRight className="text-gray-500 dark:text-gray-400" />
                                </td>
                                <td className="px-4 py-2 text-gray-600 dark:text-gray-400">
                                    Moves selected element in the specified direction
                                </td>
                            </tr>
                            <tr className="hover:bg-gray-50 dark:hover:bg-gray-600">
                                <td className="px-4 py-2 text-gray-700 dark:text-gray-300 flex items-center gap-2">
                                    <span className='font-bold'>Ctrl + Delete</span>
                                    <FaTrashAlt className="text-gray-500 dark:text-gray-400" />
                                </td>
                                <td className="px-4 py-2 text-gray-600 dark:text-gray-400">
                                    Delete selected element
                                </td>
                            </tr>
                            <tr className="hover:bg-gray-50 dark:hover:bg-gray-600">
                                <td className="px-4 py-2 text-gray-700 dark:text-gray-300 flex items-center gap-2">
                                    <span className="font-bold">Ctrl + Alt + C</span>
                                    <FaCopy className="text-gray-500 dark:text-gray-400" />
                                </td>
                                <td className="px-4 py-2 text-gray-600 dark:text-gray-400">
                                    Copy selected element
                                </td>
                            </tr>
                            <tr className="hover:bg-gray-50 dark:hover:bg-gray-600">
                                <td className="px-4 py-2 text-gray-700 dark:text-gray-300 flex items-center gap-2">
                                    <span className="font-bold">Ctrl + Alt + V</span>
                                    <FaPaste className="text-gray-500 dark:text-gray-400" />
                                </td>
                                <td className="px-4 py-2 text-gray-600 dark:text-gray-400">
                                    Paste selected element
                                </td>
                            </tr>
                        </tbody>
                    </table>
                </div>

                <div className="flex justify-between items-center mt-6">
                    <div className="text-sm text-gray-500 dark:text-gray-400">
                        Version {packageJson.version}
                    </div>
                    <button
                        className="w-fit bg-red-600 hover:bg-red-800 text-white px-4 py-2 rounded-xl"
                        onClick={handleCloseModal}
                    >
                        Close
                    </button>
                </div>
            </div>
        </div>
    );
};

export default SettingsModal;
