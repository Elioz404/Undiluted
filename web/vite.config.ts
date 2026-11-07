import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const relay = process.env.RELAY_URL ?? "http://localhost:8080";

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: { "/api": relay },
  },
});
