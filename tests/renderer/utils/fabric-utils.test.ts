import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock fabric before importing the module
vi.mock("fabric", () => {
  return {
    fabric: {
      Rect: vi.fn(() => ({
        set: vi.fn(),
        setCoords: vi.fn(),
        sendToBack: vi.fn(),
      })),
      Image: function () {},
      util: {
        createClass: vi.fn((baseClass, implementation) => {
          return function () {
            return {
              on: vi.fn(),
              scaleToHeight: vi.fn(),
              scaleToWidth: vi.fn(),
              set: vi.fn(),
              setCoords: vi.fn(),
              sendToBack: vi.fn(),
            };
          };
        }),
      },
    },
  };
});

import {
  FabricUtils,
  CoverImage,
  CoverVideo,
} from "../../../renderer/utils/fabric-utils";

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
    });

    it("should handle different padding values", () => {
      const mockEditorElement = {
        placement: {
          x: 50,
          y: 50,
          width: 200,
          height: 200,
          scaleX: 1,
          scaleY: 1,
        },
      } as any;

      const result = FabricUtils.getClipMaskRect(mockEditorElement, 5);

      expect(result).toBeDefined();
    });
  });

  describe("CoverImage", () => {
    it("should initialize with default options", () => {
      const mockElement = document.createElement("img");
      const coverImage = new CoverImage(mockElement);

      expect(coverImage).toBeDefined();
    });

    it("should initialize with custom options", () => {
      const mockElement = document.createElement("img");
      const options = { cropWidth: 100, cropHeight: 100 };
      const coverImage = new CoverImage(mockElement, options);

      expect(coverImage).toBeDefined();
    });
  });

  describe("CoverVideo", () => {
    it("should initialize with default options", () => {
      const mockElement = document.createElement("video");
      const coverVideo = new CoverVideo(mockElement);

      expect(coverVideo).toBeDefined();
    });

    it("should initialize with custom options", () => {
      const mockElement = document.createElement("video");
      const options = { cropWidth: 100, cropHeight: 100 };
      const coverVideo = new CoverVideo(mockElement, options);

      expect(coverVideo).toBeDefined();
    });
  });

  describe("FabricUtils additional methods", () => {
    it("should handle different editor element types", () => {
      const mockVideoElement = {
        placement: {
          x: 100,
          y: 200,
          width: 300,
          height: 400,
          scaleX: 1,
          scaleY: 1,
        },
        type: "video",
      } as any;

      const mockImageElement = {
        placement: {
          x: 50,
          y: 50,
          width: 200,
          height: 200,
          scaleX: 1,
          scaleY: 1,
        },
        type: "image",
      } as any;

      const result1 = FabricUtils.getClipMaskRect(mockVideoElement, 10);
      const result2 = FabricUtils.getClipMaskRect(mockImageElement, 5);

      expect(result1).toBeDefined();
      expect(result2).toBeDefined();
    });

    it("should handle text elements", () => {
      const mockTextElement = {
        placement: {
          x: 10,
          y: 20,
          width: 100,
          height: 50,
          scaleX: 1,
          scaleY: 1,
        },
        type: "text",
      } as any;

      const result = FabricUtils.getClipMaskRect(mockTextElement, 5);
      expect(result).toBeDefined();
    });

    it("should handle shape elements", () => {
      const mockShapeElement = {
        placement: {
          x: 30,
          y: 40,
          width: 80,
          height: 60,
          scaleX: 1.5,
          scaleY: 1.5,
        },
        type: "shape",
      } as any;

      const result = FabricUtils.getClipMaskRect(mockShapeElement, 8);
      expect(result).toBeDefined();
    });

    it("should handle group elements", () => {
      const mockGroupElement = {
        placement: {
          x: 0,
          y: 0,
          width: 500,
          height: 300,
          scaleX: 0.8,
          scaleY: 0.8,
        },
        type: "group",
      } as any;

      const result = FabricUtils.getClipMaskRect(mockGroupElement, 15);
      expect(result).toBeDefined();
    });

    it("should handle edge cases for placement values", () => {
      const mockElement = {
        placement: {
          x: 0,
          y: 0,
          width: 1,
          height: 1,
          scaleX: 0.5,
          scaleY: 0.5,
        },
      } as any;

      const result = FabricUtils.getClipMaskRect(mockElement, 0);

      expect(result).toBeDefined();
    });

    it("should handle negative placement values", () => {
      const mockElement = {
        placement: {
          x: -100,
          y: -200,
          width: 300,
          height: 400,
          scaleX: 1,
          scaleY: 1,
        },
      } as any;

      const result = FabricUtils.getClipMaskRect(mockElement, 10);

      expect(result).toBeDefined();
    });

    it("should handle large placement values", () => {
      const mockElement = {
        placement: {
          x: 10000,
          y: 10000,
          width: 5000,
          height: 5000,
          scaleX: 2,
          scaleY: 2,
        },
      } as any;

      const result = FabricUtils.getClipMaskRect(mockElement, 100);

      expect(result).toBeDefined();
    });

    it("should handle zero dimensions", () => {
      const mockElement = {
        placement: {
          x: 100,
          y: 200,
          width: 0,
          height: 0,
          scaleX: 1,
          scaleY: 1,
        },
      } as any;

      const result = FabricUtils.getClipMaskRect(mockElement, 10);

      expect(result).toBeDefined();
    });

    it("should handle different padding values", () => {
      const mockElement = {
        placement: {
          x: 100,
          y: 200,
          width: 300,
          height: 400,
          scaleX: 1,
          scaleY: 1,
        },
      } as any;

      const paddingValues = [0, 1, 5, 10, 50, 100];

      paddingValues.forEach((padding) => {
        const result = FabricUtils.getClipMaskRect(mockElement, padding);
        expect(result).toBeDefined();
      });
    });

    it("should handle different scale values", () => {
      const scaleValues = [0.1, 0.5, 1, 1.5, 2, 5];

      scaleValues.forEach((scale) => {
        const mockElement = {
          placement: {
            x: 100,
            y: 200,
            width: 300,
            height: 400,
            scaleX: scale,
            scaleY: scale,
          },
        } as any;

        const result = FabricUtils.getClipMaskRect(mockElement, 10);
        expect(result).toBeDefined();
      });
    });

    it("should handle asymmetric scale values", () => {
      const mockElement = {
        placement: {
          x: 100,
          y: 200,
          width: 300,
          height: 400,
          scaleX: 1.5,
          scaleY: 0.8,
        },
      } as any;

      const result = FabricUtils.getClipMaskRect(mockElement, 10);

      expect(result).toBeDefined();
    });

    it("should handle fractional placement values", () => {
      const mockElement = {
        placement: {
          x: 100.5,
          y: 200.7,
          width: 300.3,
          height: 400.9,
          scaleX: 1.2,
          scaleY: 0.8,
        },
      } as any;

      const result = FabricUtils.getClipMaskRect(mockElement, 10.5);

      expect(result).toBeDefined();
    });
  });
});
