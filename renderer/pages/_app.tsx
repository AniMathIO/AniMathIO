import React, { useEffect } from 'react'
import type { AppProps } from 'next/app'
import dynamic from 'next/dynamic'

import '../styles/globals.css'

const SettingsModal = dynamic(() => import('./SettingsModal'), { ssr: false });
const Titlebar = dynamic(() => import('./components/Titlebar'), { ssr: false });

function MyApp({ Component, pageProps }: AppProps) {
  // Load theme mode immediately on app startup (before any other component renders)
  useEffect(() => {
    const loadThemeMode = async () => {
      if (typeof window !== 'undefined' && window.electron?.ipcRenderer) {
        try {
          const savedThemeMode = await window.electron.ipcRenderer.invoke('get-theme-mode');
          document.documentElement.classList.toggle('dark', savedThemeMode === 'dark');
        } catch (error) {
          console.error('Failed to load theme mode:', error);
          // Default to light mode if there's an error
        }
      }
    };
    loadThemeMode();
  }, []);

  return (
    <div className='bg-white dark:bg-gray-900'>
      <Titlebar />
      {/* Titlebar is position:fixed (see Titlebar.tsx); page layouts size
          themselves with h-[calc(100svh-32px)] to leave room for it instead
          of h-svh, so this wrapper only needs to offset the fixed bar. */}
      <div className='pt-[32px]'>
        <Component {...pageProps} />
      </div>
      <SettingsModal />
    </div>
  );
}

export default MyApp
