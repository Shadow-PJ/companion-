import { defineConfig } from "vite";

// On-demand pages: companion, settings and AI Pulse.
export default defineConfig({
  clearScreen: false,
  server: { host: "127.0.0.1", port: 1420, strictPort: true },
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: { pet: "pet.html", settings: "settings.html", pulse: "pulse.html" },
    },
  },
});
