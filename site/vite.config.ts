import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// Served from https://ghostpengu.github.io/tokenhell/ on GitHub Pages.
export default defineConfig({
  base: process.env.SITE_BASE ?? '/tokenhell/',
  plugins: [react(), tailwindcss()],
})
