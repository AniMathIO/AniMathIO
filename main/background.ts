import path from "path";
import { exec } from "child_process";
import { IpcMainEvent, app, ipcMain, systemPreferences, dialog, BrowserWindow } from "electron";
import { readFile } from "fs/promises";
import serve from "electron-serve";
import { createWindow } from "./helpers";
import Store from "electron-store";

const store = new Store();
const isProd = process.env.NODE_ENV === "production";

// Store file path to open when app is ready
let fileToOpen: string | null = null;
let mainWindow: BrowserWindow | null = null;

if (isProd) {
  serve({ directory: "app" });
} else {
  app.setPath("userData", `${app.getPath("userData")} (development)`);
}

async function startProcess(event: IpcMainEvent, value: string) {
  if (event) {
    /*
      'parentDir' is used to get this folder -> /Applications/<youApp>.app/Contents/ 
      so that we can run our .sh file which will also launch the Python or Rust script.
      So the script folder will be moved into parentDir/ in prod mode.
      Note: this will only work if the target mahcine have Python or Rust installed.
    */
    let scriptPath;
    if (isProd) {
      const parentDir = path.dirname(path.dirname(path.dirname(__dirname)));
      scriptPath = path.join(parentDir, "scripts/runner.sh");
    } else {
      scriptPath = path.join(__dirname, "../scripts/runner.sh");
    }
    // console.log(`DEBUG: scriptPath: ${scriptPath}`)
    const cmd = `sh "${scriptPath}" ${value}`;

    exec(cmd, (error, stdout) => {
      if (error) {
        console.error(`ERROR: Error executing post-install script: ${error}`); // will be seen only dev mode, not in prod mode
        event.sender.send("log", error.message); // will be seen in both dev and prod mode (in the frontend)
        return;
      }
      event.sender.send("log", "Python script executed successfully");
      event.sender.send("message", stdout);
    });

    // ~/.yourApp.log will be helpfull to log process in production mode
  }
}

// Handle file open requests (called after app is ready)
async function handleOpenFile(filePath: string) {
  if (!mainWindow) {
    // Store for later if window isn't ready
    fileToOpen = filePath;
    return;
  }

  try {
    // Read the file
    const fileBuffer = await readFile(filePath);
    const fileName = path.basename(filePath);

    // Send to renderer process
    mainWindow.webContents.send("open-file-from-system", {
      success: true,
      data: Array.from(new Uint8Array(fileBuffer)),
      fileName: fileName,
      filePath: filePath,
    });
  } catch (error: any) {
    console.error("Error opening file:", error);
    if (mainWindow) {
      mainWindow.webContents.send("open-file-from-system", {
        success: false,
        error: error.message || "Failed to open file",
      });
    }
  }
}

(async () => {
  // Handle macOS open-file event (fires before ready)
  app.on("open-file", (event, filePath) => {
    event.preventDefault();
    fileToOpen = filePath;
    
    // If app is already ready, open it immediately
    if (app.isReady() && mainWindow) {
      handleOpenFile(filePath);
    }
  });

  // Handle Windows/Linux: prevent multiple instances and handle file opens
  const gotTheLock = app.requestSingleInstanceLock();

  if (!gotTheLock) {
    // Another instance is already running, quit this one
    app.quit();
  } else {
    // Handle second instance (when user tries to open file with already running app)
    app.on("second-instance", (event, commandLine, workingDirectory) => {
      // Focus existing window
      if (mainWindow) {
        if (mainWindow.isMinimized()) mainWindow.restore();
        mainWindow.focus();
      }

      // On Windows/Linux, file path is in commandLine
      // Format: ["path/to/electron.exe", "path/to/file.animathio"]
      if (process.platform === "win32" || process.platform === "linux") {
        const filePath = commandLine.find((arg) => arg.endsWith(".animathio"));
        if (filePath) {
          handleOpenFile(filePath);
        }
      }
    });
  }

  await app.whenReady();

  mainWindow = createWindow("main", {
    width: 1600,
    height: 900,
    minWidth: 1600,
    minHeight: 900,
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
    },
  });

  if (isProd) {
    await mainWindow.loadURL("app://./Home");
  } else {
    const port = process.argv[2];
    await mainWindow.loadURL(`http://localhost:${port}/Home`);
    // mainWindow.webContents.openDevTools();
  }

  // Handle file that was queued before app was ready
  if (fileToOpen) {
    handleOpenFile(fileToOpen);
    fileToOpen = null;
  }

  // On Windows/Linux, check process.argv for file path (when app is launched with file)
  if (process.platform === "win32" || process.platform === "linux") {
    const filePath = process.argv.find((arg) => arg.endsWith(".animathio"));
    if (filePath) {
      handleOpenFile(filePath);
    }
  }
})();

app.on("window-all-closed", () => {
  app.quit();
});

ipcMain.on("run-sh", async (event, value) => {
  console.log("DEBUG: starting process"); // for dev mode
  event.sender.send("log", "Running..."); // for prod mode
  await startProcess(event, value);
});

ipcMain.on("message", async (event, arg) => {
  event.reply("message", `${arg} World!`);
});

ipcMain.handle("get-theme-mode", () => {
  return (store as any).get("themeMode", "light");
});

ipcMain.on("set-theme-mode", (event, themeMode) => {
  (store as any).set("themeMode", themeMode);
});

ipcMain.handle("get-gemini-api-token", () => {
  return (store as any).get("geminiApiToken", "");
});

ipcMain.handle("set-gemini-api-token", (_, apiToken) => {
  (store as any).set("geminiApiToken", apiToken);
  return true;
});

// Request microphone permissions (macOS)
function requestMicrophonePermission() {
  if (process.platform === "darwin") {
    const status = systemPreferences.getMediaAccessStatus("microphone");
    if (status !== "granted") {
      systemPreferences.askForMediaAccess("microphone");
    }
  }
}

// Set up ipcMain handler for permission requests from the renderer
ipcMain.handle("request-microphone-permission", async () => {
  if (process.platform === "darwin") {
    // On macOS, we can request permission programmatically
    await systemPreferences.askForMediaAccess("microphone");
    return systemPreferences.getMediaAccessStatus("microphone");
  } else if (process.platform === "win32") {
    // Windows doesn't provide a programmatic way to request permission
    // Permissions are typically managed through Windows settings
    return "granted"; // We'll assume granted and let the API handle denial
  } else {
    // Linux and other platforms typically don't have permissions
    return "granted";
  }
});

// Handler to read project file by path (no dialog, direct read)
ipcMain.handle("read-project-file", async (event, filePath: string) => {
  try {
    // Try to read the file directly
    const fileBuffer = await readFile(filePath);
    return {
      success: true,
      data: Array.from(new Uint8Array(fileBuffer)),
      fileName: path.basename(filePath),
      filePath: filePath,
    };
  } catch (error: any) {
    // Return error without showing dialog
    return { success: false, error: error.message || "Failed to read file" };
  }
});

// Handler to open project file with dialog (for files not in history)
ipcMain.handle("open-project-file", async (event) => {
  try {
    const result = await dialog.showOpenDialog({
      title: "Open Project File",
      filters: [
        { name: "Animathio Project", extensions: ["animathio"] },
        { name: "All Files", extensions: ["*"] },
      ],
      properties: ["openFile"],
    });

    if (result.canceled || result.filePaths.length === 0) {
      return { success: false, error: "File selection cancelled" };
    }

    const selectedPath = result.filePaths[0];
    const fileBuffer = await readFile(selectedPath);
    return {
      success: true,
      data: Array.from(new Uint8Array(fileBuffer)),
      fileName: path.basename(selectedPath),
      filePath: selectedPath,
    };
  } catch (error: any) {
    return { success: false, error: error.message || "Failed to open file" };
  }
});

// Handler to save project file with dialog (returns full path)
ipcMain.handle("save-project-file", async (event, fileData: number[], suggestedName?: string) => {
  try {
    const result = await dialog.showSaveDialog({
      title: "Save Project File",
      defaultPath: suggestedName || "project.animathio",
      filters: [
        { name: "Animathio Project", extensions: ["animathio"] },
        { name: "All Files", extensions: ["*"] },
      ],
    });

    if (result.canceled || !result.filePath) {
      return { success: false, error: "File save cancelled" };
    }

    const filePath = result.filePath;
    const fileBuffer = Buffer.from(fileData);
    await require("fs/promises").writeFile(filePath, fileBuffer);

    return {
      success: true,
      fileName: path.basename(filePath),
      filePath: filePath,
    };
  } catch (error: any) {
    return { success: false, error: error.message || "Failed to save file" };
  }
});

// Handler to save project file directly to path (no dialog)
ipcMain.handle("write-project-file", async (event, filePath: string, fileData: number[]) => {
  try {
    const fileBuffer = Buffer.from(fileData);
    await require("fs/promises").writeFile(filePath, fileBuffer);
    return { success: true };
  } catch (error: any) {
    return { success: false, error: error.message || "Failed to write file" };
  }
});
