/** @type {import('next').NextConfig} */
module.exports = {
  output: "export",
  // we want to change distDir to "app" so as nextron can build the app in production mode!
  distDir: process.env.NODE_ENV === "production" ? "../app" : ".next",
  trailingSlash: true,
  images: {
    unoptimized: true,
  },
  // Next 16 defaults to Turbopack in dev; an empty object acknowledges that we are
  // not migrating the old no-op webpack hook (removed below).
  //
  // Next >= 16.4.1 Turbopack fails to build @ffmpeg/ffmpeg's dynamic
  // `new URL(classWorkerURL, import.meta.url)`; this loader strips that branch
  // (we always use the default bundled worker). See DEPENDENCIES.md.
  turbopack: {
    rules: {
      "**/@ffmpeg/ffmpeg/dist/esm/classes.js": {
        loaders: [require.resolve("./build/ffmpeg-static-worker-loader.js")],
      },
    },
  },
};
