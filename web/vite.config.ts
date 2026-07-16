import path from "path"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

const apiProxyTarget = process.env.VITE_API_PROXY_TARGET ?? "http://127.0.0.1:31990"

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": {
        target: apiProxyTarget,
        changeOrigin: true,
      },
    },
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
})
