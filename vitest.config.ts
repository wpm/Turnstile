import { defineConfig } from "vitest/config";

// Two vitest test types, one config, selected with `--project`:
//
//   vitest run --project unit         # fast frontend unit tests (src/)
//   vitest run --project scripts      # release tooling in scripts/
//   vitest run --project lean-server  # e2e: drives a real `lean --server`
//
// Playwright owns the browser e2e suite (e2e/browser/) under its own runner;
// see playwright.config.ts. Keeping the includes disjoint is what lets all
// three run without picking up each other's files.
export default defineConfig({
  test: {
    // Coverage for the unit suite, surfaced in CI and uploaded to Codecov.
    // `pnpm run test:coverage` writes coverage/lcov.info (plus a terminal
    // summary); the report is scoped to the frontend sources the unit tests
    // exercise, not the e2e/ harnesses or generated SvelteKit output.
    coverage: {
      provider: "v8",
      reporter: ["text", "lcov"],
      include: ["src/**"],
    },
    projects: [
      {
        test: {
          name: "unit",
          environment: "node",
          include: ["src/**/*.test.ts"],
        },
      },
      {
        test: {
          name: "scripts",
          environment: "node",
          include: ["scripts/**/*.test.mjs"],
        },
      },
      {
        test: {
          name: "lean-server",
          environment: "node",
          include: ["e2e/lean-server/**/*.test.mjs"],
          // Drives a real `lean --server` over stdio, so it needs far longer
          // than a unit test: 60 s per assertion, 300 s for the scenario that
          // elaborates the √2 proof in beforeAll.
          testTimeout: 60_000,
          hookTimeout: 300_000,
        },
      },
    ],
  },
});
