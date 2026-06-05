import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    // Playwright owns e2e/; vitest must not pick those specs up.
    include: ["src/**/*.test.ts"],
  },
});
