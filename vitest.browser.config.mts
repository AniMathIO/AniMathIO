import { defineConfig } from "vitest/config";
import path from "path";

export default defineConfig({
  test: {
    environment: "happy-dom",
    globals: true,
    setupFiles: ["./tests/setup.ts"],
    include: ["tests/browser/**/*.test.{ts,tsx}"],
    browser: {
      enabled: true,
      name: "chromium",
      provider: "playwright",
      headless: true,
      instances: [
        {
          browser: "chromium",
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
          branches: 60,
          functions: 70,
          lines: 70,
          statements: 70
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
