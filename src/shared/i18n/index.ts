import { useCallback } from 'react';
import { ar } from './ar';
import { en, type MessageKey } from './en';
import { useAppStore } from '../../app/store';

export type Locale = 'en' | 'ar';
export type { MessageKey };

const dictionaries: Record<Locale, Record<MessageKey, string>> = { en, ar };

export function translate(locale: Locale, key: MessageKey, vars?: Record<string, string | number>): string {
  const template = dictionaries[locale][key] ?? en[key] ?? key;
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (_, name: string) => {
    const v = vars[name];
    if (v === undefined) return `{${name}}`;
    return typeof v === 'number' ? formatNumber(locale, v) : v;
  });
}

export function formatNumber(locale: Locale, n: number): string {
  // Western digits in both languages keep page numbers and identifiers consistent with the PDFs.
  return new Intl.NumberFormat(locale === 'ar' ? 'ar-u-nu-latn' : 'en-US').format(n);
}

export function formatDate(locale: Locale, ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const sameDay = d.toDateString() === today.toDateString();
  const tag = locale === 'ar' ? 'ar-u-nu-latn' : 'en-US';
  if (sameDay) {
    return `${translate(locale, 'common.today')}, ${new Intl.DateTimeFormat(tag, { hour: '2-digit', minute: '2-digit' }).format(d)}`;
  }
  return new Intl.DateTimeFormat(tag, { year: 'numeric', month: 'short', day: 'numeric' }).format(d);
}

export function formatBytes(locale: Locale, bytes: number): string {
  const units = locale === 'ar' ? ['بايت', 'ك.ب', 'م.ب', 'غ.ب'] : ['B', 'KB', 'MB', 'GB'];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${formatNumber(locale, Math.round(v * 10) / 10)} ${units[i]}`;
}

export function useLocale(): Locale {
  return useAppStore((s) => s.locale);
}

export function useT() {
  const locale = useLocale();
  return useCallback((key: MessageKey, vars?: Record<string, string | number>) => translate(locale, key, vars), [locale]);
}
