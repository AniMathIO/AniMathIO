import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("electron", {
  ipcRenderer: {
    send: (channel: string, data: any) => ipcRenderer.send(channel, data),
    on: (channel: string, func: (...args: any[]) => void) =>
      ipcRenderer.on(channel, (event, ...args) => func(...args)),
    removeListener: (channel: string, func: (...args: any[]) => void) =>
      ipcRenderer.removeListener(channel, func),
    invoke: (channel: string, data: any) => ipcRenderer.invoke(channel, data),
  },
});
