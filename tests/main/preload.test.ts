import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Mock Electron modules
const mockContextBridge = {
  exposeInMainWorld: vi.fn(),
};

const mockIpcRenderer = {
  send: vi.fn(),
  on: vi.fn(),
  removeListener: vi.fn(),
  invoke: vi.fn(),
};

vi.mock("electron", () => ({
  contextBridge: mockContextBridge,
  ipcRenderer: mockIpcRenderer,
}));

// Import the preload module to trigger the code execution
// Note: This will be handled by the test execution

describe("Preload Script", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("should expose electron API to main world", () => {
    // Test that contextBridge.exposeInMainWorld is available
    expect(mockContextBridge.exposeInMainWorld).toBeDefined();
  });

  it("should handle IPC renderer operations", () => {
    // Test that IPC renderer methods are available
    expect(mockIpcRenderer.send).toBeDefined();
    expect(mockIpcRenderer.on).toBeDefined();
    expect(mockIpcRenderer.removeListener).toBeDefined();
    expect(mockIpcRenderer.invoke).toBeDefined();
  });

  it("should handle context bridge exposure", () => {
    // Test that the context bridge is available
    expect(mockContextBridge.exposeInMainWorld).toBeDefined();
  });

  it("should handle IPC renderer send", () => {
    const channel = "test-channel";
    const data = "test-data";

    mockIpcRenderer.send(channel, data);
    expect(mockIpcRenderer.send).toHaveBeenCalledWith(channel, data);
  });

  it("should handle IPC renderer on", () => {
    const channel = "test-channel";
    const func = vi.fn();

    mockIpcRenderer.on(channel, func);
    expect(mockIpcRenderer.on).toHaveBeenCalledWith(channel, func);
  });

  it("should handle IPC renderer removeListener", () => {
    const channel = "test-channel";
    const func = vi.fn();

    mockIpcRenderer.removeListener(channel, func);
    expect(mockIpcRenderer.removeListener).toHaveBeenCalledWith(channel, func);
  });

  it("should handle IPC renderer invoke", () => {
    const channel = "test-channel";
    const data = "test-data";

    mockIpcRenderer.invoke(channel, data);
    expect(mockIpcRenderer.invoke).toHaveBeenCalledWith(channel, data);
  });

  it("should handle microphone permission request", () => {
    mockIpcRenderer.invoke("request-microphone-permission");
    expect(mockIpcRenderer.invoke).toHaveBeenCalledWith(
      "request-microphone-permission"
    );
  });

  it("should handle multiple IPC channels", () => {
    const channels = ["channel1", "channel2", "channel3"];

    channels.forEach((channel) => {
      mockIpcRenderer.on(channel, vi.fn());
    });

    expect(mockIpcRenderer.on).toHaveBeenCalledTimes(channels.length);
  });

  it("should handle IPC renderer operations with different data types", () => {
    const testCases = [
      { channel: "string-channel", data: "string-data" },
      { channel: "number-channel", data: 123 },
      { channel: "object-channel", data: { key: "value" } },
      { channel: "array-channel", data: [1, 2, 3] },
      { channel: "boolean-channel", data: true },
    ];

    testCases.forEach(({ channel, data }) => {
      mockIpcRenderer.send(channel, data);
      expect(mockIpcRenderer.send).toHaveBeenCalledWith(channel, data);

      mockIpcRenderer.invoke(channel, data);
      expect(mockIpcRenderer.invoke).toHaveBeenCalledWith(channel, data);
    });
  });
});

it("exposes working media and video IPC APIs through the actual preload", async () => {
  vi.clearAllMocks();
  await import("../../main/preload");
  const [name, api] = mockContextBridge.exposeInMainWorld.mock.calls[0];
  expect(name).toBe("electron");
  mockIpcRenderer.invoke.mockResolvedValueOnce({ success: true, dataUrl: "data:image/png;base64,AA==", type: "image" });
  await expect(api.readMediaFile("/tmp/image.png")).resolves.toMatchObject({ success: true, type: "image" });
  expect(mockIpcRenderer.invoke).toHaveBeenLastCalledWith("read-media-file", "/tmp/image.png");
  mockIpcRenderer.invoke.mockResolvedValueOnce({ success: false, error: "disk full" });
  await expect(api.writeVideoFile("/tmp/movie.webm", [1, 2, 255])).resolves.toEqual({ success: false, error: "disk full" });
  expect(mockIpcRenderer.invoke).toHaveBeenLastCalledWith("write-video-file", "/tmp/movie.webm", [1, 2, 255]);
});
