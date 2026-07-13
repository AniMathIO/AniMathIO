/**
 * Backward-compatibility re-exports.
 * The State class is now RootStore composed of focused sub-stores.
 */
export { RootStore as State } from "./RootStore";

import { EditorElement, AudioEditorElement, VideoEditorElement, ImageEditorElement, MafsEditorElement } from "@/types";

export function isEditorAudioElement(
  element: EditorElement
): element is AudioEditorElement {
  return element.type === "audio";
}
export function isEditorVideoElement(
  element: EditorElement
): element is VideoEditorElement {
  return element.type === "video";
}
export function isEditorImageElement(
  element: EditorElement
): element is ImageEditorElement {
  return element.type === "image";
}
export function isEditorMafsElement(
  element: EditorElement
): element is MafsEditorElement {
  return element.type === "mafs";
}
