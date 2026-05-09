import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],

  // Tauri expects a fixed port for development
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Use polling to keep file watching reliable across Linux environments.
      usePolling: true,
    },
  },

  // Clear screen during dev
  clearScreen: false,

  // Environment variables prefix
  envPrefix: ['VITE_', 'TAURI_'],

  build: {
    // Linux builds run on WebKitGTK.
    target: 'safari13',
    // Don't minify for debug builds
    minify: process.env.TAURI_DEBUG ? false : 'esbuild',
    // Produce sourcemaps for debug builds
    sourcemap: !!process.env.TAURI_DEBUG,
    rollupOptions: {
      input: {
        main: 'index.html',
      },
    },
  },
})
