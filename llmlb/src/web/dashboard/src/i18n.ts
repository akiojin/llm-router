import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import LanguageDetector from 'i18next-browser-languagedetector'
import en from './locales/en.json'
import ja from './locales/ja.json'

// ダッシュボードの多言語対応（EN/JA）。言語は localStorage("llmlb-lang") に保存し、
// 未設定時はブラウザ言語から判定、最終フォールバックは英語。
// E2E は playwright.config.ts で locale=en-US を指定し常に英語で実行する。
export const SUPPORTED_LANGUAGES = ['en', 'ja'] as const
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number]

void i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      ja: { translation: ja },
    },
    supportedLngs: SUPPORTED_LANGUAGES,
    fallbackLng: 'en',
    interpolation: { escapeValue: false },
    // リソースは同期的に同梱しているため Suspense は不要。
    react: { useSuspense: false },
    detection: {
      order: ['localStorage', 'navigator'],
      lookupLocalStorage: 'llmlb-lang',
      caches: ['localStorage'],
    },
  })

export default i18n
