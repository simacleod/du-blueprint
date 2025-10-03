import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  resolve: {
    conditions: ["tauri", "browser", "module", "default"]
  },
  optimizeDeps: {
    exclude: ["@tauri-apps/api"]
  },
  server: {
    host: "0.0.0.0",
    port: 5173
  }
});
