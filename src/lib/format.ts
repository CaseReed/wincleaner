const UNITS = ["o", "Ko", "Mo", "Go", "To"] as const;

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 o";
  }
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const rounded = unit === 0 ? Math.round(value) : Math.round(value * 10) / 10;
  const text = Number.isInteger(rounded)
    ? String(rounded)
    : String(rounded).replace(".", ",");
  return `${text} ${UNITS[unit]}`;
}

const COUNT = new Intl.NumberFormat("fr-FR");

/// Compteurs de fichiers : groupés par milliers, lisibles d'un coup d'œil.
export function formatCount(count: number): string {
  return Number.isFinite(count) ? COUNT.format(Math.max(0, Math.trunc(count))) : "0";
}
