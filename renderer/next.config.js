/** @type {import('next').NextConfig} */
module.exports = {
  trailingSlash: true,
  pageExtensions: ["ts", "tsx", "js", "jsx", "md", "mdx"],
  images: {
    unoptimized: true,
  },
  webpack: (config) => {
    return config;
  },
};
