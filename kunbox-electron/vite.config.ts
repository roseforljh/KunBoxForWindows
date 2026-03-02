import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'path'
import { readFileSync } from 'fs'

const packageJson = JSON.parse(readFileSync(resolve(__dirname, 'package.json'), 'utf-8'))

export default defineConfig({
  define: {
    __APP_VERSION__: JSON.stringify(packageJson.version)
  },
  plugins: [react()],
  root: 'src/renderer',
  base: './',
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src/renderer'),
      '@renderer': resolve(__dirname, 'src/renderer'),
      '@shared': resolve(__dirname, 'src/shared')
    }
  },
  build: {
    outDir: resolve(__dirname, 'out/renderer'),
    emptyOutDir: true,
    rollupOptions: {
      input: resolve(__dirname, 'src/renderer/index.html')
    }
  }
})
