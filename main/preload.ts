import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("electron", {
  ipcRenderer: {
    send: (channel: string, data: any) => ipcRenderer.send(channel, data),
    on: (channel: string, func: (...args: any[]) => void) => {
      // Create a wrapper function that we can properly remove
      const wrapper = (event: any, ...args: any[]) => func(...args);
      ipcRenderer.on(channel, wrapper);
      // Return cleanup function
      return () => {
        ipcRenderer.removeListener(channel, wrapper);
      };
    },
    removeListener: (channel: string, func: (...args: any[]) => void) =>
      ipcRenderer.removeListener(channel, func),
    invoke: (channel: string, data: any) => ipcRenderer.invoke(channel, data),
  },
  // Add the microphone permission request function
  requestMicrophonePermission: () => 
    ipcRenderer.invoke('request-microphone-permission'),
  // Read project file by path (direct, no dialog)
  readProjectFile: (filePath: string) => 
    ipcRenderer.invoke('read-project-file', filePath),
  // Open project file with dialog (for files not in history)
  openProjectFile: () => 
    ipcRenderer.invoke('open-project-file'),
  // Save project file with dialog (returns full path)
  saveProjectFile: (fileData: number[], suggestedName?: string) => 
    ipcRenderer.invoke('save-project-file', fileData, suggestedName),
  // Write project file directly to path (no dialog)
  writeProjectFile: (filePath: string, fileData: number[]) => 
    ipcRenderer.invoke('write-project-file', filePath, fileData),
});
