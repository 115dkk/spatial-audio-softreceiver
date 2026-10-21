/// <reference types="vitest/config" />
import { defineConfig } from 'vite';

// The screen is plain TypeScript and DOM; Tauri loads `dist/` (ADR 0001).
// The CSP lives in the shell's tauri.conf.json: everything here comes from
// 'self', and styles are a file, not inline scripts.
export default defineConfig({
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  preview: { port: 4173, strictPort: true },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2022',
    // WebView2 is Chromium; nothing older needs a polyfill.
    modulePreload: { polyfill: false },
  },
  test: {
    environment: 'happy-dom',
    include: ['test/**/*.test.ts'],
    // test/i18n.test.ts renders every fixture's screens in English in one
    // file, and happy-dom keeps about 150 MB per fixture until the file ends
    // (3.7 GB at 25 fixtures), past Node's default heap on a 16 GB runner.
    execArgv: ['--max-old-space-size=8192'],
  },
});
