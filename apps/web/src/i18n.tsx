import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

export type Locale = "zh" | "en";

/** 一段同时写了中文和英文的文字。 / A piece of text written in both Chinese and English. */
export interface Text {
  readonly zh: string;
  readonly en: string;
}

interface LocaleState {
  readonly locale: Locale;
  readonly toggle: () => void;
  /** 按当前语言取文字。 / Picks the text for the current language. */
  readonly t: (text: Text) => string;
}

const STORAGE_KEY = "bamboo-web-locale";

function initialLocale(): Locale {
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved === "zh" || saved === "en") {
      return saved;
    }
  } catch {
    // 隐私模式下读不了本地存储，按浏览器语言来。
    // Private windows may block local storage; fall back to the browser language.
  }
  return window.navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

const LocaleContext = createContext<LocaleState | null>(null);

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, setLocale] = useState<Locale>(initialLocale);

  useEffect(() => {
    document.documentElement.lang = locale === "zh" ? "zh-CN" : "en";
    try {
      window.localStorage.setItem(STORAGE_KEY, locale);
    } catch {
      // 存不了也不影响使用。
      // Failing to save does not affect the page.
    }
  }, [locale]);

  const toggle = useCallback(() => setLocale((current) => (current === "zh" ? "en" : "zh")), []);
  const state = useMemo<LocaleState>(
    () => ({ locale, toggle, t: (text) => text[locale] }),
    [locale, toggle],
  );

  return <LocaleContext.Provider value={state}>{children}</LocaleContext.Provider>;
}

export function useLocale(): LocaleState {
  const state = useContext(LocaleContext);
  if (!state) {
    throw new Error("useLocale must be used inside LocaleProvider");
  }
  return state;
}

/** 按语言习惯给数字加千位分隔。 / Formats a number with the locale's thousands separator. */
export function formatNumber(value: number, locale: Locale): string {
  return value.toLocaleString(locale === "zh" ? "zh-CN" : "en-US");
}
