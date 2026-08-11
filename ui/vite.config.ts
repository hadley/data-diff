import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

export default defineConfig({
  plugins: [preact()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // The Rust server owns the API; the dev server proxies to it.
    proxy: { "/api": "http://localhost:9471" },
  },
  test: { environment: "jsdom" },
});
