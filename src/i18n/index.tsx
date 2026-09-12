import {
  createContext,
  Fragment,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { formatBytes, formatCount } from "@/lib/format";
import { en, type Dictionary, type TranslationKey } from "./en";
import { fr } from "./fr";

export type { TranslationKey } from "./en";

export type Language = "en" | "fr";
/// What the user picked in Settings. `system` is not a language: it is the
/// absence of a choice, and follows `navigator.language` for the rest of time.
export type LanguagePreference = "system" | Language;

/// Persisted the same way as every other front-end preference (the theme, the
/// update-check consent): one `wincleaner.*` key in `localStorage`, read
/// through a try/catch because a webview with site data disabled throws.
export const LANGUAGE_KEY = "wincleaner.language";

const DICTIONARIES: Record<Language, Dictionary> = { en, fr };
const LOCALES: Record<Language, string> = { en: "en-US", fr: "fr-FR" };

/// The base of a `.one` / `.other` pair, derived from the key set itself: a
/// plural key that has lost its twin stops compiling at the call site.
type PluralBase<K> = K extends `${infer B}.one` ? B : never;
export type PluralKey = PluralBase<TranslationKey>;

export type Params = Record<string, string | number>;
export type NodeParams = Record<string, ReactNode>;

function isLanguagePreference(value: unknown): value is LanguagePreference {
  return value === "system" || value === "en" || value === "fr";
}

export function readLanguagePreference(): LanguagePreference {
  try {
    const stored = localStorage.getItem(LANGUAGE_KEY);
    if (isLanguagePreference(stored)) return stored;
  } catch {
    // localStorage unavailable: follow the system, which is the default.
  }
  return "system";
}

function writeLanguagePreference(preference: LanguagePreference): void {
  try {
    localStorage.setItem(LANGUAGE_KEY, preference);
  } catch {
    // Nothing to do: the choice still holds for this session.
  }
}

/// French for a French Windows, English for everything else: there is no third
/// dictionary to fall back through.
function systemLanguage(): Language {
  const tag = typeof navigator === "undefined" ? "" : (navigator.language ?? "");
  return tag.toLowerCase().startsWith("fr") ? "fr" : "en";
}

export function resolveLanguage(preference: LanguagePreference): Language {
  return preference === "system" ? systemLanguage() : preference;
}

/// A missing parameter is left as its own placeholder rather than printed as
/// "undefined": a visible `{name}` says which key is wrong.
function interpolate(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? String(params[name]) : whole,
  );
}

/// Same substitution, but the values are React nodes: several sentences carry a
/// number that is styled (`font-mono tnum`) inside the sentence, and splitting
/// them into fragments per language is how word order gets lost.
function interpolateNodes(template: string, params: NodeParams): ReactNode[] {
  // A capturing split alternates literal, name, literal, name…
  return template.split(/\{(\w+)\}/g).map((part, index) => {
    const value = index % 2 === 1 ? (part in params ? params[part] : `{${part}}`) : part;
    return <Fragment key={index}>{value}</Fragment>;
  });
}

/// English keeps the singular for 1 only; French keeps it for 0 and 1.
function pluralForm(language: Language, count: number): "one" | "other" {
  const singular = language === "fr" ? Math.abs(count) < 2 : count === 1;
  return singular ? "one" : "other";
}

export interface I18n {
  language: Language;
  locale: string;
  preference: LanguagePreference;
  setPreference: (preference: LanguagePreference) => void;
  /// A translated string, for text nodes, `aria-label`, `title` and toasts.
  t: (key: TranslationKey, params?: Params) => string;
  /// A translated sentence whose placeholders are React nodes.
  tx: (key: TranslationKey, params: NodeParams) => ReactNode;
  /// `t` on the `.one` / `.other` pair `key`; `count` is also interpolated as
  /// `{count}`, already grouped for the locale.
  tn: (key: PluralKey, count: number, params?: Params) => string;
  /// `tx` on the same pair.
  txn: (key: PluralKey, count: number, params?: NodeParams) => ReactNode;
  formatBytes: (bytes: number) => string;
  formatCount: (count: number) => string;
}

function createI18n(
  preference: LanguagePreference,
  setPreference: (preference: LanguagePreference) => void,
): I18n {
  const language = resolveLanguage(preference);
  const locale = LOCALES[language];
  const dictionary = DICTIONARIES[language];
  const count = (n: number) => formatCount(n, locale);
  const t = (key: TranslationKey, params?: Params) => interpolate(dictionary[key], params);
  const tx = (key: TranslationKey, params: NodeParams) =>
    interpolateNodes(dictionary[key], params);
  const pluralKey = (key: PluralKey, n: number) =>
    `${key}.${pluralForm(language, n)}` as TranslationKey;
  return {
    language,
    locale,
    preference,
    setPreference,
    t,
    tx,
    tn: (key, n, params) => t(pluralKey(key, n), { count: count(n), ...params }),
    txn: (key, n, params) => tx(pluralKey(key, n), { count: count(n), ...params }),
    formatBytes: (bytes: number) => formatBytes(bytes, locale),
    formatCount: count,
  };
}

/// English outside a provider: that is what the component tests render, and it
/// is also the honest fallback if the provider is ever forgotten.
const I18nContext = createContext<I18n>(createI18n("en", () => {}));

export function I18nProvider({ children }: { children: ReactNode }) {
  const [preference, setStoredPreference] = useState<LanguagePreference>(readLanguagePreference);

  const setPreference = useCallback((next: LanguagePreference) => {
    writeLanguagePreference(next);
    setStoredPreference(next);
  }, []);

  const value = useMemo(() => createI18n(preference, setPreference), [preference, setPreference]);

  /// The lang attribute is what tells a screen reader which voice to read the
  /// interface with, and CSS hyphenation which rules to apply.
  useEffect(() => {
    document.documentElement.lang = value.language;
  }, [value.language]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18n {
  return useContext(I18nContext);
}
