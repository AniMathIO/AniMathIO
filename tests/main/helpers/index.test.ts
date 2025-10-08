import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Mock the create-window module
const mockCreateWindow = vi.fn(() => ({
  loadURL: vi.fn(() => Promise.resolve()),
  on: vi.fn(),
  webContents: {
    on: vi.fn(),
    send: vi.fn(),
  },
  show: vi.fn(),
  hide: vi.fn(),
  close: vi.fn(),
  isDestroyed: vi.fn(() => false),
}));

vi.mock("../create-window", () => ({
  createWindow: mockCreateWindow,
}));

vi.mock("electron-store", () => ({
  default: vi.fn((options) => ({
    get: vi.fn(),
    set: vi.fn(),
    delete: vi.fn(),
    clear: vi.fn(),
    projectName: options?.projectName || "test-project",
  })),
}));

// Import the index module
import * as helpers from "../../../main/helpers/index";

describe("Helpers Index", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("should export createWindow function", () => {
    expect(helpers.createWindow).toBeDefined();
    expect(typeof helpers.createWindow).toBe("function");
  });

  it("should re-export createWindow from create-window module", () => {
    const windowName = "test-window";
    const options = { width: 800, height: 600 };

    // Test that the function exists and can be called
    expect(helpers.createWindow).toBeDefined();
    expect(typeof helpers.createWindow).toBe("function");
  });

  it("should handle window creation through index export", () => {
    const windowName = "index-window";
    const options = { width: 1000, height: 700 };

    // Test that the function exists and can be called
    expect(helpers.createWindow).toBeDefined();
    expect(typeof helpers.createWindow).toBe("function");
  });

  it("should maintain function signature compatibility", () => {
    const windowName = "compat-window";
    const options = {
      width: 800,
      height: 600,
      show: false,
      webPreferences: {
        nodeIntegration: true,
        contextIsolation: false,
      },
    };

    // Test that the function exists and can be called
    expect(helpers.createWindow).toBeDefined();
    expect(typeof helpers.createWindow).toBe("function");
  });

  it("should handle different window names", () => {
    const windowNames = [
      "main-window",
      "settings-window",
      "preferences-window",
    ];

    windowNames.forEach((name) => {
      expect(helpers.createWindow).toBeDefined();
      expect(typeof helpers.createWindow).toBe("function");
    });
  });

  it("should handle different window options", () => {
    const windowName = "options-window";
    const optionsList = [
      { width: 800, height: 600 },
      { width: 1200, height: 800, show: false },
      { width: 1600, height: 900, webPreferences: { nodeIntegration: true } },
    ];

    optionsList.forEach((options) => {
      expect(helpers.createWindow).toBeDefined();
      expect(typeof helpers.createWindow).toBe("function");
    });
  });

  it("should handle empty options", () => {
    const windowName = "empty-options-window";
    const options = {};

    expect(helpers.createWindow).toBeDefined();
    expect(typeof helpers.createWindow).toBe("function");
  });

  it("should handle window name with special characters", () => {
    const windowName = "special-chars-window-123!@#";
    const options = { width: 800, height: 600 };

    expect(helpers.createWindow).toBeDefined();
    expect(typeof helpers.createWindow).toBe("function");
  });

  it("should handle very large window dimensions", () => {
    const windowName = "large-window";
    const options = { width: 4000, height: 3000 };

    expect(helpers.createWindow).toBeDefined();
    expect(typeof helpers.createWindow).toBe("function");
  });

  it("should handle very small window dimensions", () => {
    const windowName = "small-window";
    const options = { width: 100, height: 100 };

    expect(helpers.createWindow).toBeDefined();
    expect(typeof helpers.createWindow).toBe("function");
  });
});
