import { defineConfig, globalIgnores } from "eslint/config";
import nextCoreWebVitals from "eslint-config-next/core-web-vitals";
import nextTypescript from "eslint-config-next/typescript";
import tseslint from "typescript-eslint";

export default defineConfig([
  globalIgnores([
    "app/**",
    "dist/**",
    "renderer/.next/**",
    "renderer/out/**",
    "**/node_modules/**",
    "coverage/**",
    "coverage-browser/**",
    "playwright-report/**",
    "test-results/**",
    "repo/**",
    "build-dir/**",
    "renderer/public/**",
    "**/*.d.ts",
  ]),
  ...nextCoreWebVitals,
  ...nextTypescript,
  tseslint.configs.recommended,
  {
    settings: {
      // Next.js lives in renderer/, not the repo root
      next: { rootDir: "renderer" },
    },
    rules: {
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
          caughtErrorsIgnorePattern: "^_",
        },
      ],
    },
  },
  {
    // CommonJS scripts legitimately use require().
    files: ["**/*.cjs", "scripts/**/*.js"],
    rules: { "@typescript-eslint/no-require-imports": "off" },
  },
  {
    // Test doubles/mocks routinely need `any` to reach into private members
    // and stub partial Electron/Konva/DOM objects; enforced for app code only.
    files: ["tests/**/*.{ts,tsx}"],
    rules: { "@typescript-eslint/no-explicit-any": "off" },
  },
]);
