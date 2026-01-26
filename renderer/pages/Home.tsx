"use client";

import React, { useState, useEffect } from 'react';
import Editor from './Editor';
import Dashboard from './Dashboard';
import { StateContext } from '@/states';
import { State } from '../states/state';
import { addProjectToHistory } from '@/utils';
import { ProjectLoadingModal } from './components/partials/ProjectLoadingModal';

const HomePage = () => {
  // Create a single shared state instance
  const [state] = useState(() => new State());

  useEffect(() => {
    // Handle files opened from system (Open with)
    if (typeof window !== 'undefined' && window.electron?.onOpenFileFromSystem) {
      const unsubscribe = window.electron.onOpenFileFromSystem(async (fileData) => {
        if (!fileData.success) {
          console.error('Failed to open file from system:', fileData.error);
          state.setProjectLoadingStatus('error', fileData.error || 'Unknown error');
          return;
        }

        try {
          // Set loading state
          state.setProjectLoadingStatus('loading', 'Loading project...');
          state.setProjectLoadingProgress(0);

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

          // Set success state
          state.setProjectLoadingStatus('success', '');
          
          // Auto-close success modal after 1.5 seconds
          setTimeout(() => {
            if (state.projectLoadingStatus === 'success') {
              state.setProjectLoadingStatus('idle', '');
            }
          }, 1500);
        } catch (error) {
          console.error('Error loading project from system:', error);
          state.setProjectLoadingStatus(
            'error',
            error instanceof Error ? error.message : 'Unknown error'
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
      <ProjectLoadingModal />
      <Dashboard />
      <Editor />
    </StateContext.Provider>
  );
}

export default HomePage;
