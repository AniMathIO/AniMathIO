"use client";

import React, { useState } from 'react';
import Editor from './Editor';
import Dashboard from './Dashboard';
import { StateContext } from '@/states';
import { State } from '../states/state';

const HomePage = () => {
  // Create a single shared state instance
  const [state] = useState(() => new State());
  
  return (
    <StateContext.Provider value={state}>
      <Dashboard />
      <Editor />
    </StateContext.Provider>
  );
}

export default HomePage;
