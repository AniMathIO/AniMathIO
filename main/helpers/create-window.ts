import {
  screen,
  BrowserWindow,
  BrowserWindowConstructorOptions,
  Rectangle,
  Menu,
  shell,
  ipcMain,
} from "electron";
import Store from "electron-store";
import path from "path";

export const createWindow = (
  windowName: string,
  options: BrowserWindowConstructorOptions
): BrowserWindow => {
  const key = "window-state";
  const name = `window-state-${windowName}`;
  const store = new Store<Rectangle>({ name });
  const defaultSize = {
    width: options.width || 1600,
    height: options.height || 900,
    x: 0,
    y: 0,
  };
  let state = {};

  const restore = () => store.get(key, defaultSize);

  const getCurrentPosition = () => {
    const position = win.getPosition();
    const size = win.getSize();
    return {
      x: position[0],
      y: position[1],
      width: size[0],
      height: size[1],
    };
  };

  const windowWithinBounds = (windowState: Rectangle, bounds: Rectangle) => {
    return (
      windowState.x >= bounds.x &&
      windowState.y >= bounds.y &&
      windowState.x + windowState.width <= bounds.x + bounds.width &&
      windowState.y + windowState.height <= bounds.y + bounds.height
    );
  };

  const resetToDefaults = () => {
    const bounds = screen.getPrimaryDisplay().bounds;
    return Object.assign({}, defaultSize, {
      x: (bounds.width - (defaultSize.width || 1600)) / 2,
      y: (bounds.height - (defaultSize.height || 900)) / 2,
    });
  };

  const ensureVisibleOnSomeDisplay = (windowState: Rectangle) => {
    const visible = screen.getAllDisplays().some((display) => {
      return windowWithinBounds(windowState, display.bounds);
    });
    if (!visible) {
      // Window is partially or fully not visible now.
      // Reset it to safe defaults.
      return resetToDefaults();
    }
    return windowState;
  };

  const saveState = () => {
    if (!win.isMinimized() && !win.isMaximized()) {
      Object.assign(state, getCurrentPosition());
    }
    store.set(key, state);
  };

  state = ensureVisibleOnSomeDisplay(restore());

  const win = new BrowserWindow({
    ...state,
    ...options,
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      preload: path.join(__dirname, "preload.js"),
      ...options.webPreferences,
    },
  });

  const menuTemplate: Electron.MenuItemConstructorOptions[] = [
    {
      label: "File",
      submenu: [
        {
          label: "Quit",
          accelerator: "CommandOrControl+Q",
          click: () => {
            win.close();
          },
        },
      ],
    },
    {
      label: "Settings",
      submenu: [
        {
          label: "Open Settings",
          click: () => {
            win.webContents.send("open-settings-modal");
          },
        },
      ],
    },
    {
      label: "Help",
      submenu: [
        {
          label: "Website",
          click: () => {
            shell.openExternal("https://animathio.com");
          },
        },
        {
          label: "Documentation",
          click: () => {
            shell.openExternal("https://docs.animathio.com/");
          },
        },
        {
          label: "Discord",
          click: () => {
            shell.openExternal("https://discord.com/invite/cZMTYSAHRX");
          },
        },
      ],
    },
    // {
    //   label: "View",
    //   submenu: [
    //     {
    //       role: "toggleDevTools",
    //     },
    //   ],
    // },
  ];

  const menu = Menu.buildFromTemplate(menuTemplate);

  win.setMenu(menu);

  win.on("close", saveState);

  return win;
};
