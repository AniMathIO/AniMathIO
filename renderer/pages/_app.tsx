import React from 'react'
import type { AppProps } from 'next/app'
import dynamic from 'next/dynamic'

import '../styles/globals.css'

const SettingsModal = dynamic(() => import('./SettingsModal'), { ssr: false });

function MyApp({ Component, pageProps }: AppProps) {
  return (
    <div className='bg-white dark:bg-gray-900'>
      <Component {...pageProps} />
      <SettingsModal />
    </div>
  );
}

export default MyApp
