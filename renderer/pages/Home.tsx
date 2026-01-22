"use client";

import React, { useState, useEffect } from 'react';
import Editor from './Editor';
import Dashboard from './Dashboard';
import { StateContext } from '@/states';
import { State } from '../states/state';
import { addProjectToHistory } from '@/utils';

const HomePage = () => {
  // Create a single shared state instance
  const [state] = useState(() => new State());

  useEffect(() => {
    // Handle files opened from system (Open with)
    if (typeof window !== 'undefined' && window.electron?.onOpenFileFromSystem) {
      const unsubscribe = window.electron.onOpenFileFromSystem(async (fileData) => {
        if (!fileData.success) {
          console.error('Failed to open file from system:', fileData.error);
          alert(`Failed to open project: ${fileData.error || 'Unknown error'}`);
          return;
        }

        try {
          // Convert array back to ArrayBuffer
          const buffer = new Uint8Array(fileData.data || []).buffer;

          // Deserialize the project
          await state.deserialize(buffer);

          // Store file information
          if (fileData.fileName) {
            state.setCurrentProjectFileName(fileData.fileName);
          }
          if (fileData.filePath) {
            state.setCurrentProjectFilePath(fileData.filePath);
          }
          state.setCurrentProjectFileHandle(null); // File handle not available through IPC

          // Add to project history with current timestamp
          if (fileData.filePath && fileData.fileName) {
            await addProjectToHistory(fileData.filePath, fileData.fileName);
          }

          // Activate editor
          state.setEditorActive(true);
          state.setSelectedMenuOption("Videos");
        } catch (error) {
          console.error('Error loading project from system:', error);
          alert(
            `Failed to load project: ${
              error instanceof Error ? error.message : 'Unknown error'
            }`
          );
        }
      });

      // Cleanup on unmount
      return () => {
        if (unsubscribe) {
          unsubscribe();
        }
      };
    }
  }, [state]);
  
  return (
    <StateContext.Provider value={state}>
      <Dashboard />
      <Editor />
    </StateContext.Provider>
  );
}

export default HomePage;
