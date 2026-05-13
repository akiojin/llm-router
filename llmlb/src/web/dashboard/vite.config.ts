import { defineConfig, type ProxyOptions } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

const devApiProxy: ProxyOptions = {
  target: 'http://localhost:32768',
  changeOrigin: true,
  configure: (proxy) => {
    proxy.on('proxyReq', (proxyReq, req) => {
      const forwardedHost = req.headers.host
      if (forwardedHost) {
        proxyReq.setHeader('x-forwarded-host', forwardedHost)
      }
      proxyReq.setHeader('x-forwarded-proto', 'http')
    })
  },
}

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  build: {
    outDir: '../static',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, 'index.html'),
        login: path.resolve(__dirname, 'login.html'),
        register: path.resolve(__dirname, 'register.html'),
        'change-password': path.resolve(__dirname, 'change-password.html'),
      },
      output: {
        // Ensure consistent file names for Rust include_dir!
        entryFileNames: 'assets/[name]-[hash].js',
        chunkFileNames: 'assets/[name]-[hash].js',
        assetFileNames: 'assets/[name]-[hash].[ext]',
      },
    },
  },
  base: '/dashboard/',
  server: {
    proxy: {
      '/api': devApiProxy,
      '/v1': devApiProxy,
    },
  },
})
