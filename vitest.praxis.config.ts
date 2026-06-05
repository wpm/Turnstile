import { defineConfig } from "vitest/config";

// Praxis suite: drives a real `lean --server`, so it gets its own config
// with long timeouts and is excluded from the default `pnpm test` run.
export default defineConfig({
  test: {
    environment: "node",
    include: ["praxis/**/*.praxis.test.mjs"],
    testTimeout: 60_000,
    hookTimeout: 300_000,
  },
});
