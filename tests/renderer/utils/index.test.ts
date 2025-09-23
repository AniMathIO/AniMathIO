import { describe, it, expect } from "vitest";
import {
  getUid,
  isHtmlVideoElement,
  isHtmlImageElement,
  isHtmlAudioElement,
  formatTimeToMinSec,
  formatTimeToMinSecMili,
} from "../../../renderer/utils";

describe("Utils", () => {
  describe("getUid", () => {
    it("should generate a unique ID", () => {
      const id1 = getUid();
      const id2 = getUid();
      
      expect(id1).toBeDefined();
      expect(id2).toBeDefined();
      expect(id1).not.toBe(id2);
      expect(typeof id1).toBe("string");
      expect(id1.length).toBeGreaterThan(0);
    });
  });

  describe("isHtmlVideoElement", () => {
    it("should return true for video elements", () => {
      const videoElement = { tagName: "VIDEO" } as HTMLVideoElement;
      expect(isHtmlVideoElement(videoElement)).toBe(true);
    });

    it("should return false for non-video elements", () => {
      const imageElement = { tagName: "IMG" } as HTMLImageElement;
      const audioElement = { tagName: "AUDIO" } as HTMLAudioElement;
      const divElement = { tagName: "DIV" } as HTMLElement;
      
      expect(isHtmlVideoElement(imageElement)).toBe(false);
      expect(isHtmlVideoElement(audioElement)).toBe(false);
      expect(isHtmlVideoElement(divElement)).toBe(false);
    });

    it("should return false for null elements", () => {
      expect(isHtmlVideoElement(null)).toBe(false);
    });
  });

  describe("isHtmlImageElement", () => {
    it("should return true for image elements", () => {
      const imageElement = { tagName: "IMG" } as HTMLImageElement;
      expect(isHtmlImageElement(imageElement)).toBe(true);
    });

    it("should return false for non-image elements", () => {
      const videoElement = { tagName: "VIDEO" } as HTMLVideoElement;
      const audioElement = { tagName: "AUDIO" } as HTMLAudioElement;
      const divElement = { tagName: "DIV" } as HTMLElement;
      
      expect(isHtmlImageElement(videoElement)).toBe(false);
      expect(isHtmlImageElement(audioElement)).toBe(false);
      expect(isHtmlImageElement(divElement)).toBe(false);
    });

    it("should return false for null elements", () => {
      expect(isHtmlImageElement(null)).toBe(false);
    });
  });

  describe("isHtmlAudioElement", () => {
    it("should return true for audio elements", () => {
      const audioElement = { tagName: "AUDIO" } as HTMLAudioElement;
      expect(isHtmlAudioElement(audioElement)).toBe(true);
    });

    it("should return false for non-audio elements", () => {
      const videoElement = { tagName: "VIDEO" } as HTMLVideoElement;
      const imageElement = { tagName: "IMG" } as HTMLImageElement;
      const divElement = { tagName: "DIV" } as HTMLElement;
      
      expect(isHtmlAudioElement(videoElement)).toBe(false);
      expect(isHtmlAudioElement(imageElement)).toBe(false);
      expect(isHtmlAudioElement(divElement)).toBe(false);
    });

    it("should return false for null elements", () => {
      expect(isHtmlAudioElement(null)).toBe(false);
    });
  });

  describe("formatTimeToMinSec", () => {
    it("should format time correctly", () => {
      expect(formatTimeToMinSec(0)).toBe("0:00");
      expect(formatTimeToMinSec(30)).toBe("0:30");
      expect(formatTimeToMinSec(60)).toBe("1:00");
      expect(formatTimeToMinSec(90)).toBe("1:30");
      expect(formatTimeToMinSec(3661)).toBe("61:01");
    });
  });

  describe("formatTimeToMinSecMili", () => {
    it("should format time with milliseconds correctly", () => {
      expect(formatTimeToMinSecMili(0)).toBe("0:00.00");
      expect(formatTimeToMinSecMili(1000)).toBe("0:01.00");
      expect(formatTimeToMinSecMili(1500)).toBe("0:01.50");
      expect(formatTimeToMinSecMili(61000)).toBe("1:01.00");
    });
  });
});
