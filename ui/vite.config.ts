// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/vite.config.ts
//  Author : Praneeth
//  License: MIT
// =============================================================================
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import type { Plugin } from "vite";

/**
 * Dev only: component styles are injected by each component's JS (see
 * emitCss below), so a request for "<file>.svelte?svelte&type=style" can only
 * come from a stale module cached by the app's webview from an older session.
 * Answer it with an empty stylesheet instead of letting Vite feed the raw
 * .svelte file to PostCSS ("Unknown word SysPulse" / "Unknown word value").
 */
function staleSvelteCss(): Plugin {
  const isStyle = (id: string) => id.includes(".svelte?svelte&type=style");
  return {
    name: "syspulse:stale-svelte-css",
    apply: "serve",
    enforce: "pre",
    resolveId(id) {
      return isStyle(id) ? "\0stale-svelte-style.css" : null;
    },
    load(id) {
      return id === "\0stale-svelte-style.css" ? "" : null;
    },
  };
}

// Tauri expects a fixed port and must not clear the terminal output.
export default defineConfig(({ command }) => ({
  // Dev: inject component CSS from the component's JS instead of separate
  // "?svelte&type=style" modules. After a dev-server restart the webview can
  // request those CSS modules before the component is compiled, and Vite then
  // feeds the raw .svelte file to PostCSS ("Unknown word SysPulse") and the
  // page renders without styles. Production builds keep extracted CSS.
  plugins: [staleSvelteCss(), svelte({ emitCss: command === "build" })],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: "es2021",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
}));
