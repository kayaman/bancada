import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["src/**/__tests__/**/*.test.{ts,tsx}"],
    environment: "node",
    // Without this, Vitest short-circuits every CSS import to an empty string,
    // including `styles.css?raw`. theme/__tests__/cssDefaults.test.ts reads the
    // real stylesheet to hold its :root defaults to the TypeScript they
    // duplicate, and cannot do that against "".
    css: true,
  },
});
