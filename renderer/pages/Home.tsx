"use client";

import React, { useState, useEffect } from 'react';
import Editor from './Editor';
import Dashboard from './Dashboard';
import { StateContext } from '@/states';
import { RootStore as State } from '../states/RootStore';
import { addProjectToHistory } from '@/utils';
import { ProjectLoadingModal } from './components/partials/ProjectLoadingModal';
import { createMcpDispatcher } from '@/mcp/dispatcher';

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

  useEffect(() => {
    // The MCP server runs in the main process but every tool acts on this store,
    // so commands arrive here and the reply is paired by id on the way back.
    if (typeof window === 'undefined' || !window.electron?.ipcRenderer) return;

    const dispatch = createMcpDispatcher(state);
    const unsubscribe = window.electron.ipcRenderer.on(
      'mcp-command',
      async (command: { id: string; tool: string; args: Record<string, unknown> }) => {
        if (!command?.id) return;
        try {
          const data = await dispatch(command.tool, command.args ?? {});
          window.electron.ipcRenderer.send('mcp-result', { id: command.id, ok: true, data });
        } catch (error) {
          window.electron.ipcRenderer.send('mcp-result', {
            id: command.id,
            ok: false,
            error: error instanceof Error ? error.message : String(error),
          });
        }
      }
    );

    return () => {
      if (typeof unsubscribe === 'function') unsubscribe();
    };
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
