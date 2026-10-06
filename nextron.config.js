/**
 * The MCP SDK must not be bundled into the main-process build.
 *
 * Its Node HTTP transport wraps @hono/node-server, which does its own
 * environment detection and resolves pieces dynamically; webpack inlines all of
 * that and the result fails at runtime (every request answered with a 500)
 * while working fine when required normally. Both packages are in
 * `dependencies`, so electron-builder ships them and Electron can require them
 * from node_modules at runtime -- the same arrangement electron-store and
 * electron-serve already rely on.
 */
const EXTERNAL_AT_RUNTIME = [/^@modelcontextprotocol\/sdk/, /^@hono\/node-server/];

module.exports = {
  webpack: (config) => {
    const existing = config.externals
      ? Array.isArray(config.externals)
        ? config.externals
        : [config.externals]
      : [];

    config.externals = [
      ...existing,
      ({ request }, callback) => {
        if (request && EXTERNAL_AT_RUNTIME.some((pattern) => pattern.test(request))) {
          return callback(null, `commonjs ${request}`);
        }
        return callback();
      },
    ];

    return config;
  },
};
