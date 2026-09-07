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
  turbopack: {},
};
