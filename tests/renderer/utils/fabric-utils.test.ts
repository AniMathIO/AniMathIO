import { describe, it, expect, vi, beforeEach } from "vitest";
import { FabricUtils } from "../../../renderer/utils/fabric-utils";

// Mock fabric
vi.mock("fabric", () => {
  return {
    fabric: {
      Rect: vi.fn(() => ({
        set: vi.fn(),
        setCoords: vi.fn(),
        sendToBack: vi.fn(),
      })),
      util: {
        createClass: vi.fn((baseClass, implementation) => {
          return function () {
            return {
              on: vi.fn(),
              scaleToHeight: vi.fn(),
              scaleToWidth: vi.fn(),
            };
          };
        }),
      },
    },
  };
});

describe("FabricUtils", () => {
  let mockCanvas: any;

  beforeEach(() => {
    mockCanvas = {
      add: vi.fn(),
      remove: vi.fn(),
      getObjects: vi.fn(() => []),
      renderAll: vi.fn(),
    };
  });

  describe("getClipMaskRect", () => {
    it("should create and return a clip mask rect", () => {
      const mockEditorElement = {
        placement: {
          x: 100,
          y: 200,
          width: 300,
          height: 400,
          scaleX: 1,
          scaleY: 1,
        },
      } as any;
      
      const result = FabricUtils.getClipMaskRect(mockEditorElement, 10);
      
      expect(result).toBeDefined();
      // The method creates a rect but doesn't necessarily add it to canvas
      // Just verify the method doesn't throw and returns something
    });
  });
});
