import { defineConfig } from 'vitest/config'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { playwright } from '@vitest/browser-playwright'
import tailwindcss from '@tailwindcss/vite'

// Tauri uses WebKit on macOS (WKWebView) and Linux (WebKitGTK), but
// Chromium-based WebView2 on Windows. Match the test browser to the runtime.
const tauriBrowser = process.platform === 'win32' ? 'chromium' : 'webkit'

// https://vite.dev/config/
export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  // Prevent Vite from clearing the terminal so Tauri output can interleave
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: !process.env.TAURI_ENV_DEBUG ? 'oxc' : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
  test: {
    coverage: {
      provider: 'istanbul',
      reporter: ['text', 'lcov'],
      reportsDirectory: './coverage',
    },
    // Split unit tests (jsdom, fast) from component tests (real browser via
    // Playwright). Browser tests exercise Svelte 5 components in the same
    // engine Tauri uses; unit tests run in jsdom with no browser.
    projects: [
      {
        extends: true,
        test: {
          name: 'unit',
          environment: 'jsdom',
          include: ['src/**/*.test.ts'],
          exclude: ['src/**/*.svelte.test.ts'],
        },
      },
      {
        extends: true,
        test: {
          name: 'browser',
          include: ['src/**/*.svelte.test.ts'],
          browser: {
            enabled: true,
            provider: playwright(),
            headless: true,
            instances: [{ browser: tauriBrowser }],
          },
        },
      },
    ],
  },
})
