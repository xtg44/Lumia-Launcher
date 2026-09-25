import { createI18n } from 'vue-i18n'
import zh from './zh'
import zhTW from './zh-TW'
import zhHK from './zh-HK'
import en from './en'
import ja from './ja'
import fr from './fr'
import ko from './ko'
import type { MessageSchema } from './zh'

export type AppLocale = 'zh-CN' | 'zh-TW' | 'zh-HK' | 'en' | 'ja' | 'fr' | 'ko'

export const i18n = createI18n<[MessageSchema], AppLocale>({
  legacy: false,
  locale: 'zh-CN',
  fallbackLocale: 'zh-CN',
  messages: { 'zh-CN': zh, 'zh-TW': zhTW, 'zh-HK': zhHK, en, ja, fr, ko },
})

/**
 * 根据设置偏好与系统语言决定实际语言：
 * - 设置了 zh-CN / zh-TW / zh-HK / en / ja / fr / ko → 直接用
 * - auto → 系统语言 zh 开头用中文（hk→粵語、tw/hant→台灣正體，其餘簡體）、ja→日語、ko→韓語，其餘英文
 */
export function resolveLocale(pref: string, systemLocale: string): AppLocale {
  if (['zh-CN', 'zh-TW', 'zh-HK', 'en', 'ja', 'fr', 'ko'].includes(pref)) return pref as AppLocale
  const sys = systemLocale.toLowerCase().replace('_', '-')
  if (sys.startsWith('zh')) {
    // 香港偏好 zh-HK，台灣偏好 zh-TW，其餘簡體
    if (sys.includes('hk')) return 'zh-HK'
    if (sys.includes('tw') || sys.includes('hant')) return 'zh-TW'
    return 'zh-CN'
  }
  if (sys.startsWith('ja')) return 'ja'
  if (sys.startsWith('ko')) return 'ko'
  if (sys.startsWith('fr')) return 'fr'
  return 'en'
}

/** 应用当前语言（同时同步到 i18n 实例与 <html lang>） */
export function applyLocale(locale: AppLocale) {
  // vue-i18n 的类型把 global.locale 推断成字面量联合，但运行时是 WritableComputedRef，
  // 这里通过受控断言绕过类型怪癖（参见 createI18n<[MessageSchema], AppLocale> 的泛型解析）
  ;(i18n.global.locale as unknown as { value: AppLocale }).value = locale
  document.documentElement.lang = locale
}
