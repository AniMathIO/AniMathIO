import React from 'react'
import type { AppProps } from 'next/app'

import '../styles/globals.css'
import SettingsModal from './SettingsModal';

function MyApp({ Component, pageProps }: AppProps) {
  return (
    <>
      <Component {...pageProps} />
      <SettingsModal /> {/* Add this line */}
    </>
  );
}

export default MyApp
