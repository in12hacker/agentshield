import { relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import { checkI18n } from './scripts/check-i18n.mjs';

const src = fileURLToPath(new URL('./src/', import.meta.url));

export default defineConfig({
  test: { include: ['src/**/*.test.{ts,tsx}'] },
  plugins: [{
    name: 'agentshield-i18n',
    configResolved(config) {
      if (!config.isPreview) checkI18n();
    },
    async hotUpdate({ file, read, type }) {
      const path = relative(src, file);
      if (this.environment.name !== 'client' || path.startsWith('..')
        || !/\.(tsx?|json)$/.test(path)) return;
      if (type !== 'delete') await read();
      checkI18n();
    },
  }],
});
