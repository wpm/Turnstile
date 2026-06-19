import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  // Browser specs only. The Lean LSP-contract suite under e2e/lean-server/
  // is a Node/vitest test (the "lean-server" project in vitest.config.ts),
  // not Playwright.
  testDir: "e2e/browser",
  fullyParallel: false,
  workers: 1,
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
