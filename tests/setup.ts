import "@testing-library/jest-dom";
import { vi } from "vitest";

// Mock Electron APIs
Object.defineProperty(window, 'electron', {
  value: {
    ipcRenderer: {
      invoke: vi.fn(),
      on: vi.fn(),
      off: vi.fn(),
    },
  },
  writable: true,
});
