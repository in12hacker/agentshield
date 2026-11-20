import ja from './locales/ja-JP.json' with { type: 'json' };
import zh from './locales/zh-CN.json' with { type: 'json' };
import en from './locales/en-US.json' with { type: 'json' };
import { locales, messagePlaceholder, type Locale } from './i18n-contract';

export type MessageKey = keyof typeof ja;
export type MessageParams = Readonly<Record<string, string | number>>;
export interface Message { readonly key: MessageKey; readonly params?: MessageParams }
export const dictionaries: Record<Locale, Record<MessageKey, string>> = { 'ja-JP': ja, 'zh-CN': zh, 'en-US': en };
export const isLocale = (value: unknown): value is Locale => locales.some(locale => locale === value);
export const message = (key: MessageKey, params?: MessageParams): Message => params ? { key, params } : { key };

// One strict presentation entry. No locale/key/parameter fallback and no evidence translation.
export function translate(locale: Locale, key: MessageKey, params: MessageParams = {}): string {
  const template = dictionaries[locale][key];
  if (typeof template !== 'string' || !template.trim()) throw new Error(`i18n_missing:${locale}:${key}`);
  const expected = new Set(Array.from(template.matchAll(messagePlaceholder), match => match[1]!));
  if (expected.size !== Object.keys(params).length || Object.keys(params).some(name => !expected.has(name))) {
    throw new Error(`i18n_parameters:${key}`);
  }
  return template.replace(messagePlaceholder, (_, name: string) => String(params[name]));
}
