import React from 'react'
import type { AppProps } from 'next/app'

import '../styles/globals.css'
import SettingsModal from './SettingsModal';

function MyApp({ Component, pageProps }: AppProps) {
  return (
    <div className='bg-white dark:bg-gray-900'>
      <Component {...pageProps} />
      <SettingsModal /> {/* Add this line */}
    </div>
  );
}

export default MyApp
