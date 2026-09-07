import { describe, it, expect, afterEach } from "vitest";
import { waitForElementById, waitForMediaReady } from "../../../renderer/utils/domLoad";

describe("waitForElementById", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("resolves immediately if the element already exists", async () => {
    const el = document.createElement("div");
    el.id = "already-here";
    document.body.appendChild(el);

    const found = await waitForElementById("already-here");
    expect(found).toBe(el);
  });

  it("resolves once a matching element is appended later", async () => {
    const promise = waitForElementById("appears-later");

    setTimeout(() => {
      const el = document.createElement("div");
      el.id = "appears-later";
      document.body.appendChild(el);
    }, 20);

    const found = await promise;
    expect(found?.id).toBe("appears-later");
  });

  it("resolves to null if the element never appears within the timeout", async () => {
    const found = await waitForElementById("never-appears", 30);
    expect(found).toBeNull();
  });
});

describe("waitForMediaReady", () => {
  it("resolves immediately for an already-complete image", async () => {
    const img = document.createElement("img");
    Object.defineProperty(img, "complete", { value: true, configurable: true });
    Object.defineProperty(img, "naturalWidth", { value: 200, configurable: true });

    await expect(waitForMediaReady(img)).resolves.toBeUndefined();
  });

  it("resolves an image once its load event fires", async () => {
    const img = document.createElement("img");
    Object.defineProperty(img, "complete", { value: false, configurable: true });
    Object.defineProperty(img, "naturalWidth", { value: 0, configurable: true });

    const promise = waitForMediaReady(img);
    img.dispatchEvent(new Event("load"));

    await expect(promise).resolves.toBeUndefined();
  });

  it("resolves (does not hang) an image on an error event", async () => {
    const img = document.createElement("img");
    Object.defineProperty(img, "complete", { value: false, configurable: true });
    Object.defineProperty(img, "naturalWidth", { value: 0, configurable: true });

    const promise = waitForMediaReady(img);
    img.dispatchEvent(new Event("error"));

    await expect(promise).resolves.toBeUndefined();
  });

  it("resolves immediately for a video with metadata already loaded", async () => {
    const video = document.createElement("video");
    Object.defineProperty(video, "readyState", { value: 1, configurable: true });

    await expect(waitForMediaReady(video)).resolves.toBeUndefined();
  });

  it("resolves a video once loadedmetadata fires", async () => {
    const video = document.createElement("video");
    Object.defineProperty(video, "readyState", { value: 0, configurable: true });

    const promise = waitForMediaReady(video);
    video.dispatchEvent(new Event("loadedmetadata"));

    await expect(promise).resolves.toBeUndefined();
  });

  it("resolves an audio element once loadedmetadata fires", async () => {
    const audio = document.createElement("audio");
    Object.defineProperty(audio, "readyState", { value: 0, configurable: true });

    const promise = waitForMediaReady(audio);
    audio.dispatchEvent(new Event("loadedmetadata"));

    await expect(promise).resolves.toBeUndefined();
  });
});
