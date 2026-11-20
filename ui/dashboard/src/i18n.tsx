import { createContext, useContext, useLayoutEffect, useState, type ReactNode } from 'react';
import { Languages } from 'lucide-react';
import { defaultLocale, locales, type Locale } from './i18n-contract';
import { isLocale, translate, type MessageKey, type MessageParams } from './messages';

const preferenceKey = 'agentshield.dashboard.language';
function initialLocale(): Locale {
  try { const saved = localStorage.getItem(preferenceKey); if (isLocale(saved)) return saved; }
  catch { /* A blocked preference store does not prevent rendering. Credentials never enter this store. */ }
  return defaultLocale;
}
const I18nContext = createContext<{ locale: Locale; setLocale: (locale: Locale) => void } | null>(null);
export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, updateLocale] = useState<Locale>(initialLocale);
  useLayoutEffect(() => {
    document.documentElement.lang = locale;
    document.title = translate(locale, 'app.title');
  }, [locale]);
  function setLocale(next: Locale) {
    updateLocale(next);
    try { localStorage.setItem(preferenceKey, next); } catch { /* In-memory language remains usable. */ }
  }
  return <I18nContext.Provider value={{ locale, setLocale }}>{children}</I18nContext.Provider>;
}
export function useI18n() {
  const context = useContext(I18nContext);
  if (!context) throw new Error('i18n_provider_required');
  return { ...context, t: (key: MessageKey, params?: MessageParams) => translate(context.locale, key, params) };
}
const languageKeys: Record<Locale, MessageKey> = { 'ja-JP': 'language.ja', 'zh-CN': 'language.zh', 'en-US': 'language.en' };
export function LanguageSelect() {
  const { locale, setLocale, t } = useI18n();
  return <label className="language-select" title={t('language.select')}>
    <Languages size={18} aria-hidden="true" />
    <select aria-label={t('language.select')} value={locale} onChange={event => {
      if (isLocale(event.target.value)) setLocale(event.target.value);
    }}>
      {locales.map(value => <option key={value} value={value} lang={value}>{t(languageKeys[value])}</option>)}
    </select>
  </label>;
}
