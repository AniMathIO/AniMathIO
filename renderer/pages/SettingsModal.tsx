import React, { useState, useEffect } from 'react';
import { SunIcon, MoonIcon, XMarkIcon } from '@heroicons/react/24/solid';
const SettingsModal: React.FC = () => {
    const [isOpen, setIsOpen] = useState(false);
    const [isDarkMode, setIsDarkMode] = useState(false);

    useEffect(() => {
        const handleOpenModal = () => {
            setIsOpen(true);
        };


    }, []);

    const handleCloseModal = () => {
        setIsOpen(false);
    };

    const handleToggleDarkMode = () => {
        setIsDarkMode(!isDarkMode);
        document.documentElement.classList.toggle('dark');
    };

    if (!isOpen) {
        return null;
    }

    return (
        <div className="fixed inset-0 flex items-center justify-center z-50">
            <div className="absolute inset-0 bg-black opacity-70"></div>
            <div className="flex flex-col relative w-[400px] bg-white dark:bg-gray-800 rounded-lg p-6">
                <h2 className="text-2xl font-bold mb-4 dark:text-white">Settings</h2>
                <div className="flex items-center mb-4">
                    <label className="mr-2 dark:text-white">Dark Mode:</label>
                    {/* Dark mode toggle with heroicons */}
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