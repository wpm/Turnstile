import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  // Browser specs only. The Lean LSP-contract suite under e2e/lean-server/
  // is a Node/vitest test (the "lean-server" project in vitest.config.ts),
  // not Playwright.
  testDir: "e2e/browser",
  // Each test runs in its own browser context, and the fake backend
  // (src/lib/fake/) keeps its state in module-level globals scoped to that
  // page — so no two tests, in any project, share backend state. That makes
  // parallel execution safe: Playwright treats every (project × spec) as an
  // independent unit, so the chromium and webkit projects run concurrently
  // instead of one engine finishing before the other starts.
  fullyParallel: true,
  // Playwright defaults workers to 1 under CI; pin it so the chromium and
  // webkit projects actually run side by side there too. 2 matches the
  // GitHub-hosted runner's core count — enough to overlap the engines without
  // oversubscribing. Locally, omitting this would let Playwright scale to the
  // machine, but a fixed value keeps CI and local timing predictable.
  workers: 2,
  timeout: 30_000,
  use: {
    baseURL: "http://127.0.0.1:4173",
  },
  // Run every spec in both engines. Chromium is the fast default; WebKit is the
  // engine behind the macOS Tauri webview (WKWebView), so it's the only one
  // that reproduces WebKit-specific layout quirks — e.g. the flex-column
  // transcript handing its rows a stretched height, which clipped or ballooned
  // the assistant chat bubbles. Without a WebKit project those regressions pass
  // in CI and only surface in the shipped app.
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
    {
      name: "webkit",
      use: { ...devices["Desktop Safari"] },
    },
  ],
  webServer: {
    command:
      "node node_modules/vite/bin/vite.js dev --mode e2e --port 4173 --strictPort",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: true,
    timeout: 60_000,
  },
});
