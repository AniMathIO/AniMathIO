import { defineConfig } from "vitest/config";
import path from "path";

export default defineConfig({
  test: {
    environment: "happy-dom",
    globals: true,
    setupFiles: ["./tests/setup.ts"],
    include: ["tests/**/*.test.{ts,tsx}"],
    browser: {
      enabled: true,
      name: "chromium",
      provider: "playwright",
      headless: true,
      instances: [
        {
          browser: "chromium",
          launch: {
            args: ["--no-sandbox", "--disable-setuid-sandbox"]
          },
          context: {
            viewport: { width: 1280, height: 720 }
          }
        }
      ]
    },
    coverage: {
      provider: "v8",
      reporter: ["text", "json", "html", "lcov"],
      reportsDirectory: "./coverage",
        include: [
          "renderer/states/**/*.ts",
          "renderer/utils/**/*.ts",
          "main/**/*.ts"
        ],
      exclude: [
        // Next.js build artifacts and chunks
        "**/app/**",
        "**/dist/**",
        "**/.next/**",
        "**/node_modules/**",
        "**/coverage/**",
        
        // Next.js specific files
        "**/next.config.js",
        "**/next-env.d.ts",
        "**/postcss.config.js",
        
        // Build and config files
        "**/electron-builder*.yml",
        "**/tsconfig.json",
        "**/vitest.config.mts",
        
        // Generated files
        "**/*.d.ts",
        "**/*.map",
        "**/*.js.map",
        
        // Test files themselves (to avoid double counting)
        "**/*.test.{ts,tsx}",
        "**/*.spec.{ts,tsx}",
        
        // Static assets
        "**/public/**",
        "**/resources/**",
        "**/scripts/**",
        
        // Coverage reports
        "**/coverage/**",
        "**/coverage-final.json"
      ],
      // Set thresholds for unit tests only
      thresholds: {
        global: {
          branches: 80,
          functions: 85,
          lines: 85,
          statements: 85
        }
      },
      // Enable all coverage options
      all: true,
      // Skip files that are not covered
      skipFull: false
    },
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./renderer"),
    },
  },
});
