import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { checkI18n } from './check-i18n.mjs';

function fixture(context) {
  const root = mkdtempSync(join(tmpdir(), 'as-i18n-check-'));
  const src = join(root, 'src');
  mkdirSync(join(src, 'locales'), { recursive: true });
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const catalog = (locale, value) => writeFileSync(join(src, 'locales', `${locale}.json`), JSON.stringify(value));
  catalog('ja-JP', { action: '更新', count: '{number} 件' });
  catalog('zh-CN', { action: '刷新', count: '{number} 条' });
  catalog('en-US', { action: 'Refresh', count: '{number} items' });
  const component = value => writeFileSync(join(src, 'App.tsx'), value);
  component("<button aria-label={t('action')} className=\"compact primary\">{t('count', { number: 2 })}</button>");
  return { root, catalog, component };
}

test('accepts complete catalogs and translated JSX', context => {
  assert.equal(checkI18n(fixture(context).root), 2);
});

test('rejects a missing Japanese key', context => {
  const { root, catalog } = fixture(context);
  catalog('ja-JP', { action: '更新' });
  assert.throws(() => checkI18n(root), /ja-JP: missing count/);
});

test('rejects inconsistent interpolation parameters', context => {
  const { root, catalog } = fixture(context);
  catalog('en-US', { action: 'Refresh', count: '{total} items' });
  assert.throws(() => checkI18n(root), /en-US: parameters differ for count/);
});

test('rejects literal text and accessible labels', context => {
  const { root, component } = fixture(context);
  component('<button aria-label="Refresh">更新</button>');
  assert.throws(() => checkI18n(root), error =>
    /literal visible attribute/.test(error.message) && /literal UI text/.test(error.message));
});
