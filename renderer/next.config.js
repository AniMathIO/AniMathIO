/** @type {import('next').NextConfig} */
module.exports = {
  trailingSlash: true,
  pageExtensions: ["states.ts", "utils.ts", "state.ts", "fabric-utils.ts"],
  images: {
    unoptimized: true,
  },
  webpack: (config) => {
    return config;
  },
};
