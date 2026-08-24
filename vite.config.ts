import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { fileURLToPath, URL } from 'node:url'

// Tauri 开发要求：固定端口 1420、禁用清屏（避免覆盖 Rust 编译日志）
export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Rust 目录变更由 cargo 监听，vite 忽略以免重复触发
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    // Tauri 面向系统 WebView，无需为旧浏览器降级
    target: 'esnext',
    sourcemap: false,
  },
})
