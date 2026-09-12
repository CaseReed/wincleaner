const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;
/// French counts in octets, and a French Windows says "Go", never "GB".
const UNITS_FR = ["o", "Ko", "Mo", "Go", "To"] as const;

/// `Intl.NumberFormat` is not cheap to build, and these two run once per rule
/// row: two locales times two precisions is the whole table.
const FORMATTERS = new Map<string, Intl.NumberFormat>();

function numberFormat(
  locale: string,
  maximumFractionDigits: number,
  useGrouping: boolean,
): Intl.NumberFormat {
  const key = `${locale}:${maximumFractionDigits}:${useGrouping}`;
  let formatter = FORMATTERS.get(key);
  if (!formatter) {
    formatter = new Intl.NumberFormat(locale, { maximumFractionDigits, useGrouping });
    FORMATTERS.set(key, formatter);
  }
  return formatter;
}

/// The locale decides the decimal mark and the unit: 1536 bytes is "1.5 KB" in
/// English and "1,5 Ko" in French. Never grouped: the mantissa stays under
/// 1024, and a thousands mark on a four-digit byte count reads as a decimal
/// mark in French.
export function formatBytes(bytes: number, locale = "en-US"): string {
  const units = locale.startsWith("fr") ? UNITS_FR : UNITS;
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return `0 ${units[0]}`;
  }
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 ? 0 : 1;
  return `${numberFormat(locale, digits, false).format(value)} ${units[unit]}`;
}

/// File counters: grouped by thousands the way the locale groups them,
/// readable at a glance.
export function formatCount(count: number, locale = "en-US"): string {
  return Number.isFinite(count)
    ? numberFormat(locale, 0, true).format(Math.max(0, Math.trunc(count)))
    : "0";
}
