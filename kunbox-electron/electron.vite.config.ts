import { defineConfig } from 'electron-vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { readFileSync } from 'fs'
import { resolve } from 'path'

// 渲染层 App.tsx / About.tsx 直接引用 __APP_VERSION__，不注入就是白屏报错
const define = {
  __APP_VERSION__: JSON.stringify(JSON.parse(readFileSync(resolve('package.json'), 'utf-8')).version)
}

export default defineConfig({
  main: {
    define,
    // electron-vite 6 默认开启 build.externalizeDeps，无需再使用废弃的 externalizeDepsPlugin
    resolve: {
      alias: {
        '@main': resolve('src/main'),
        '@shared': resolve('src/shared')
      }
    }
  },
  preload: {
    define,
    resolve: {
      alias: {
        '@shared': resolve('src/shared')
      }
    }
  },
  renderer: {
    define,
    plugins: [react(), tailwindcss()],
    resolve: {
      alias: {
        '@': resolve('src/renderer'),
        '@renderer': resolve('src/renderer'),
        '@shared': resolve('src/shared')
      }
    }
  }
})
