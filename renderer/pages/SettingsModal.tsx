import React, { useState, useEffect } from 'react';
import { SunIcon, MoonIcon } from '@heroicons/react/24/solid';
import { FaArrowLeft, FaArrowRight, FaArrowUp, FaArrowDown, FaTrashAlt, FaCopy, FaPaste } from 'react-icons/fa';
declare global {
    interface Window {
        electron: {
            ipcRenderer: {
                send(channel: string, data: any): void;
                on(channel: string, func: (...args: any[]) => void): () => void;
                removeListener(channel: string, func: (...args: any[]) => void): void;
                invoke(channel: string, data?: any): Promise<any>;
            };
        };
    }
}

const SettingsModal: React.FC = () => {
    const [isOpen, setIsOpen] = useState(false);
    const [isDarkMode, setIsDarkMode] = useState(false);

    useEffect(() => {
        const handleOpenModal = () => {
            setIsOpen(true);
        };

        const unsubscribe = window.electron.ipcRenderer.on('open-settings-modal', handleOpenModal);

        // Load the saved theme mode when the component mounts
        const loadThemeMode = async () => {
            const savedThemeMode = await window.electron.ipcRenderer.invoke('get-theme-mode');
            setIsDarkMode(savedThemeMode === 'dark');
            document.documentElement.classList.toggle('dark', savedThemeMode === 'dark');
        };

        loadThemeMode();

        return () => {
            unsubscribe();
        };
    }, []);

    const handleCloseModal = () => {
        setIsOpen(false);
    };

    const handleToggleDarkMode = () => {
        const newThemeMode = !isDarkMode ? 'dark' : 'light';
        setIsDarkMode(!isDarkMode);
        document.documentElement.classList.toggle('dark');
        // Save the updated theme mode
        window.electron.ipcRenderer.send('set-theme-mode', newThemeMode);
    };

    if (!isOpen) {
        return null;
    }

    return (
        <div className="fixed inset-0 flex items-center justify-center z-50">
            <div className="absolute inset-0 bg-black opacity-70"></div>
            <div className="flex flex-col relative w-[650px] bg-white dark:bg-gray-800 rounded-lg p-6">
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
                                    <span className='font-bold'>Delete or Backspace</span>
                                    <FaTrashAlt className="text-gray-500 dark:text-gray-400" />
                                </td>
                                <td className="px-4 py-2 text-gray-600 dark:text-gray-400">
                                    Delete selected element
                                </td>
                            </tr>
                            <tr className="hover:bg-gray-50 dark:hover:bg-gray-600">
                                <td className="px-4 py-2 text-gray-700 dark:text-gray-300 flex items-center gap-2">
                                    <span className="font-bold">Ctrl + C</span>
                                    <FaCopy className="text-gray-500 dark:text-gray-400" />
                                </td>
                                <td className="px-4 py-2 text-gray-600 dark:text-gray-400">
                                    Copy selected element
                                </td>
                            </tr>
                            <tr className="hover:bg-gray-50 dark:hover:bg-gray-600">
                                <td className="px-4 py-2 text-gray-700 dark:text-gray-300 flex items-center gap-2">
                                    <span className="font-bold">Ctrl + V</span>
                                    <FaPaste className="text-gray-500 dark:text-gray-400" />
                                </td>
                                <td className="px-4 py-2 text-gray-600 dark:text-gray-400">
                                    Paste selected element
                                </td>
                            </tr>
                        </tbody>
                    </table>
                </div>

                <button
                    className="w-fit self-end bg-red-600 hover:bg-red-800 text-white px-4 py-2 rounded-xl"
                    onClick={handleCloseModal}
                >
                    Close
                </button>
            </div>
        </div>
    );
};

export default SettingsModal;
