import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import path from 'path'

// arch-review [H7]: ダッシュボードのユニットテスト基盤（vitest + jsdom +
// @testing-library）。純粋関数・フックの回帰を Rust 側の include_str! ではなく
// フロントエンド自身のテストで担保する。
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  test: {
    environment: 'jsdom',
    globals: false,
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
  },
})
