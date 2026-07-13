"use client";
import React, { useState, useRef, useEffect } from "react";
import AniMathIO from "../../public/images/AniMathIO.png";

/**
 * In-app fallback for the native Electron menu (main/helpers/create-window.ts).
 * Native menu-bar rendering is unreliable on some Linux/Wayland setups (no
 * visible menu at all), so this mirrors the same File/Settings/Help actions
 * as a normal React component that's guaranteed to render regardless of OS/DE.
 */

type MenuKey = "file" | "settings" | "help" | null;

const Titlebar = () => {
  const [openMenu, setOpenMenu] = useState<MenuKey>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpenMenu(null);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  const toggleMenu = (key: MenuKey) => {
    setOpenMenu((current) => (current === key ? null : key));
  };

  const openSettings = () => {
    window.dispatchEvent(new Event("open-settings-modal"));
    setOpenMenu(null);
  };

  const openExternal = (url: string) => {
    window.electron?.openExternalUrl(url);
    setOpenMenu(null);
  };

  const quit = () => {
    window.close();
    setOpenMenu(null);
  };

  return (
    <div
      ref={containerRef}
      className="fixed top-0 left-0 right-0 z-[100] flex items-center bg-white dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700 h-[32px] px-2 text-sm select-none"
    >
      <img src={AniMathIO.src} alt="AniMathIO" className="h-4 w-4 mr-2" />
      <div className="relative">
        <button
          className="px-2 py-1 rounded-sm hover:bg-slate-200 dark:hover:bg-gray-700 text-black dark:text-white"
          onClick={() => toggleMenu("file")}
        >
          File
        </button>
        {openMenu === "file" && (
          <div className="absolute left-0 top-full z-50 min-w-[160px] bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-sm shadow-lg py-1">
            <button
              className="w-full text-left px-3 py-1.5 hover:bg-slate-200 dark:hover:bg-gray-700 text-black dark:text-white"
              onClick={quit}
            >
              Quit
            </button>
          </div>
        )}
      </div>
      <div className="relative">
        <button
          className="px-2 py-1 rounded-sm hover:bg-slate-200 dark:hover:bg-gray-700 text-black dark:text-white"
          onClick={() => toggleMenu("settings")}
        >
          Settings
        </button>
        {openMenu === "settings" && (
          <div className="absolute left-0 top-full z-50 min-w-[160px] bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-sm shadow-lg py-1">
            <button
              className="w-full text-left px-3 py-1.5 hover:bg-slate-200 dark:hover:bg-gray-700 text-black dark:text-white"
              onClick={openSettings}
            >
              Open Settings
            </button>
          </div>
        )}
      </div>
      <div className="relative">
        <button
          className="px-2 py-1 rounded-sm hover:bg-slate-200 dark:hover:bg-gray-700 text-black dark:text-white"
          onClick={() => toggleMenu("help")}
        >
          Help
        </button>
        {openMenu === "help" && (
          <div className="absolute left-0 top-full z-50 min-w-[160px] bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-sm shadow-lg py-1">
            <button
              className="w-full text-left px-3 py-1.5 hover:bg-slate-200 dark:hover:bg-gray-700 text-black dark:text-white"
              onClick={() => openExternal("https://animathio.com")}
            >
              Website
            </button>
            <button
              className="w-full text-left px-3 py-1.5 hover:bg-slate-200 dark:hover:bg-gray-700 text-black dark:text-white"
              onClick={() => openExternal("https://docs.animathio.com/")}
            >
              Documentation
            </button>
            <button
              className="w-full text-left px-3 py-1.5 hover:bg-slate-200 dark:hover:bg-gray-700 text-black dark:text-white"
              onClick={() => openExternal("https://discord.com/invite/cZMTYSAHRX")}
            >
              Discord
            </button>
          </div>
        )}
      </div>
    </div>
  );
};

export default Titlebar;
