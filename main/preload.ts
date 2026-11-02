import { contextBridge, ipcRenderer, type IpcRendererEvent } from "electron";

contextBridge.exposeInMainWorld("electron", {
  ipcRenderer: {
    send: (channel: string, data: unknown) => ipcRenderer.send(channel, data),
    on: (channel: string, func: (...args: unknown[]) => void) => {
      // Create a wrapper function that we can properly remove
      const wrapper = (_event: IpcRendererEvent, ...args: unknown[]) => func(...args);
      ipcRenderer.on(channel, wrapper);
      // Return cleanup function
      return () => {
        ipcRenderer.removeListener(channel, wrapper);
      };
    },
    removeListener: (channel: string, func: (...args: unknown[]) => void) =>
      ipcRenderer.removeListener(channel, func),
    invoke: (channel: string, data: unknown) => ipcRenderer.invoke(channel, data),
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
  // Listen for file opened from system (Open with)
  readMediaFile: (filePath: string) => ipcRenderer.invoke('read-media-file', filePath),
  writeVideoFile: (filePath: string, fileData: number[]) =>
    ipcRenderer.invoke('write-video-file', filePath, fileData),
  // Listen for file opened from system (Open with)
  onOpenFileFromSystem: (callback: (data: unknown) => void) => {
    const wrapper = (_event: IpcRendererEvent, data: unknown) => callback(data);
    ipcRenderer.on('open-file-from-system', wrapper);
    // Return cleanup function
    return () => {
      ipcRenderer.removeListener('open-file-from-system', wrapper);
    };
  },
  // Open external URL
  openExternalUrl: (url: string) => 
    ipcRenderer.invoke('open-external-url', url),
});
