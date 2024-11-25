/** @type {import('next').NextConfig} */
module.exports = {
  output: "export",
  // we want to change distDir to "app" so as nextron can build the app in production mode!
  distDir: process.env.NODE_ENV === "production" ? "../app" : ".next",
  trailingSlash: true,
  images: {
    unoptimized: true,
  },
  webpack: (config) => {
    return config;
  },
};
