import { makeAutoObservable } from "mobx";
import type { RootStore } from "../RootStore";

export class MediaStore {
  videos: string[] = [];
  images: string[] = [];
  audios: string[] = [];

  constructor(private root: RootStore) {
    makeAutoObservable(this);
  }

  setVideos(videos: string[]) {
    this.videos = videos;
  }

  setImages(images: string[]) {
    this.images = images;
  }

  setAudios(audios: string[]) {
    this.audios = audios;
  }

  addVideoResource(video: string) {
    this.videos = [...this.videos, video];
  }

  addAudioResource(audio: string) {
    this.audios = [...this.audios, audio];
  }

  addImageResource(image: string) {
    this.images = [...this.images, image];
  }

  replaceImageResource(index: number, newImageUrl: string) {
    const oldImageUrl = this.images[index];
    this.images[index] = newImageUrl;

    // Update editor elements that reference the old image URL
    for (const element of this.root.elementStore.editorElements) {
      if (
        element.type === "image" &&
        "properties" in element &&
        "src" in element.properties &&
        element.properties.src === oldImageUrl
      ) {
        element.properties.src = newImageUrl;
      }
    }

    // Revoke old blob URL if no longer in use
    const isStillInUse =
      this.images.includes(oldImageUrl) ||
      this.root.elementStore.editorElements.some(
        (el) =>
          el.type === "image" &&
          "properties" in el &&
          "src" in el.properties &&
          el.properties.src === oldImageUrl
      );

    if (!isStillInUse && oldImageUrl.startsWith("blob:")) {
      URL.revokeObjectURL(oldImageUrl);
    }
  }
}
