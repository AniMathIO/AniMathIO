import "@testing-library/jest-dom";
import { vi } from "vitest";

// Mock process for browser environment (needed for Next.js Image component)
if (typeof globalThis.process === 'undefined') {
  globalThis.process = {
    env: {
      NODE_ENV: 'test',
    },
  } as NodeJS.Process;
}

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
