import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: 'es2022',
    chunkSizeWarningLimit: 1200,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules/antd') || id.includes('node_modules/@ant-design') || id.includes('node_modules/@rc-component') || id.includes('node_modules/rc-')) {
            return 'antd-vendor';
          }
        },
      },
    },
  },
});
