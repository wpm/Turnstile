import { defineConfig, devices } from '@playwright/test'

// Tauri uses WebKit on macOS (WKWebView) and Linux (WebKitGTK), but
// Chromium-based WebView2 on Windows. Match the test browser to the runtime.
const isWindows = process.platform === 'win32'
const projectName = isWindows ? 'chromium' : 'webkit'
const deviceProfile = isWindows ? devices['Desktop Chrome'] : devices['Desktop Safari']

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: 'list',
  use: {
    baseURL: 'http://localhost:1420',
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: projectName,
      use: { ...deviceProfile },
    },
  ],
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:1420',
    reuseExistingServer: !process.env.CI,
    timeout: 30000,
  },
})
