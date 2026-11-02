/**
 * Turbopack loader for `@ffmpeg/ffmpeg/dist/esm/classes.js`.
 *
 * `FFmpeg#load()` contains
 *
 *   classWorkerURL ? new Worker(new URL(classWorkerURL, import.meta.url), ...)
 *                  : new Worker(new URL("./worker.js", import.meta.url), ...)
 *
 * Since Next 16.4.1 Turbopack hard-fails the build on the first, fully dynamic
 * `new URL(<variable>, import.meta.url)` ("Module not found: Can't resolve
 * (<dynamic> | 'undefined')"). We never pass `classWorkerURL` (the bundled
 * default worker is what we want), so drop the dynamic branch and keep only the
 * statically analysable one. Fails loudly if the upstream source changes shape,
 * so a @ffmpeg/ffmpeg bump can't silently skip the patch.
 */
const DYNAMIC_BRANCH = /classWorkerURL\s*\?\s*new Worker\(new URL\(classWorkerURL, import\.meta\.url\), \{\s*type: "module",\s*\}\)\s*:/;

module.exports = function ffmpegStaticWorkerLoader(source) {
  if (!DYNAMIC_BRANCH.test(source)) {
    throw new Error(
      "ffmpeg-static-worker-loader: expected dynamic classWorkerURL branch not found in @ffmpeg/ffmpeg classes.js; " +
        "the package changed -- re-check whether this loader is still needed."
    );
  }
  return source.replace(DYNAMIC_BRANCH, "");
};
