import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";

const host = /** @type {string | undefined} */ (process.env["TAURI_DEV_HOST"]);

// https://vite.dev/config/
export default defineConfig(({ mode }) => ({
  plugins: [tailwindcss(), sveltekit()],

  // In `e2e` mode the Tauri IPC modules are replaced by an in-browser fake
  // backend (src/lib/fake/) so Playwright can drive the real UI in a plain
  // browser. `vite dev --mode e2e` or `vite build --mode e2e`.
  resolve:
    mode === "e2e"
      ? {
          alias: {
            "@tauri-apps/api/core": "/src/lib/fake/core.ts",
            "@tauri-apps/api/event": "/src/lib/fake/event.ts",
          },
        }
      : undefined,

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      // 4. ignore SvelteKit's auto-generated files — they change during startup
      //    and cause spurious full-page reloads that wipe the editor content
      ignored: ["**/src-tauri/**", "**/.svelte-kit/generated/**"],
    },
  },
}));
