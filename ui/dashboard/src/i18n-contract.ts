export const locales = ['ja-JP', 'zh-CN', 'en-US'] as const;
export type Locale = typeof locales[number];
export const defaultLocale: Locale = locales[0];
export const messagePlaceholder = /\{([A-Za-z][A-Za-z0-9_]*)\}/g;
