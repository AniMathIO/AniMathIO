import { defineConfig } from "vitest/config";
import { playwright } from "@vitest/browser-playwright";
import path from "path";

export default defineConfig({
  // tsconfig.json sets jsx: "preserve" for Next.js's own SWC-based JSX transform.
  // Vite's dev server needs to transform JSX itself to serve these files to the
  // browser instance; without this override it fails to parse every .tsx file
  // (Vite logs a cosmetic "oxc options will be used, esbuild options ignored"
  // warning, but removing this option reproduces the parse failures, so the
  // warning doesn't reflect what's actually happening).
  esbuild: {
    jsx: "automatic",
  },
  test: {
    environment: "happy-dom",
    globals: true,
    setupFiles: ["./tests/setup.ts"],
    include: ["tests/browser/**/*.test.{ts,tsx}"],
    browser: {
      enabled: true,
      name: "chromium",
      provider: playwright(),
      headless: true,
      instances: [
        {
          browser: "chromium",
          // @ts-expect-error - launch option is supported at runtime but not in types
          launch: {
            args: [
              "--no-sandbox",
              "--disable-setuid-sandbox",
              "--disable-dev-shm-usage",
              "--disable-gpu",
              "--disable-web-security",
              "--allow-running-insecure-content"
            ]
          },
          context: {
            viewport: { width: 1280, height: 720 },
            userAgent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
          }
        }
      ]
    },
    coverage: {
      provider: "v8",
      reporter: ["text", "json", "html", "lcov"],
      reportsDirectory: "./coverage-browser",
      include: [
        "renderer/**/*.{ts,tsx}",
        "main/**/*.ts"
      ],
      exclude: [
        "**/node_modules/**",
        "**/dist/**",
        "**/coverage/**",
        "**/*.test.{ts,tsx}",
        "**/*.spec.{ts,tsx}",
        "**/app/**",
        "**/.next/**"
      ],
      thresholds: {
        global: {
          branches: 50,
          functions: 50,
          lines: 50,
          statements: 50
        }
      }
    }
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./renderer"),
    },
  },
});
