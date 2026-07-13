/**
 * Waits for an element with the given id to mount in the DOM. Used from plain
 * event handlers (not React components) where the MutationObserver-based
 * polling pattern in Editor.tsx's useDomElementById hook can't be reused
 * directly. Resolves to null on timeout rather than hanging forever, so a
 * failed drop can't stall the caller indefinitely.
 */
export function waitForElementById(id: string, timeoutMs = 5000): Promise<HTMLElement | null> {
  return new Promise((resolve) => {
    const existing = document.getElementById(id);
    if (existing) {
      resolve(existing);
      return;
    }

    const observer = new MutationObserver(() => {
      const found = document.getElementById(id);
      if (found) {
        clearTimeout(timer);
        observer.disconnect();
        resolve(found);
      }
    });
    observer.observe(document.body, { childList: true, subtree: true });

    const timer = setTimeout(() => {
      observer.disconnect();
      resolve(document.getElementById(id));
    }, timeoutMs);
  });
}

/**
 * Waits for a media element to have real decoded data available (not just be
 * mounted) — video/audio need loadedmetadata (readyState >= HAVE_METADATA),
 * images need a completed decode. Resolves (never rejects) on an error event
 * too, so a broken dropped file can't hang the caller.
 */
export function waitForMediaReady(el: HTMLVideoElement | HTMLImageElement | HTMLAudioElement): Promise<void> {
  return new Promise((resolve) => {
    if (el instanceof HTMLImageElement) {
      if (el.complete && el.naturalWidth > 0) {
        resolve();
        return;
      }
      el.addEventListener("load", () => resolve(), { once: true });
      el.addEventListener("error", () => resolve(), { once: true });
      return;
    }

    if (el.readyState >= 1) {
      resolve();
      return;
    }
    el.addEventListener("loadedmetadata", () => resolve(), { once: true });
    el.addEventListener("error", () => resolve(), { once: true });
  });
}
