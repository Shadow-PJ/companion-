import { defineConfig } from "vite";

// Two pages: the pet window and the settings window.
export default defineConfig({
  clearScreen: false,
  server: { host: "127.0.0.1", port: 1420, strictPort: true },
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: { pet: "pet.html", settings: "settings.html" },
    },
  },
});
