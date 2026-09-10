const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const rounded = unit === 0 ? Math.round(value) : Math.round(value * 10) / 10;
  return `${rounded} ${UNITS[unit]}`;
}

const COUNT = new Intl.NumberFormat("en-US");

/// File counters: grouped by thousands with a comma, readable at a glance.
export function formatCount(count: number): string {
  return Number.isFinite(count) ? COUNT.format(Math.max(0, Math.trunc(count))) : "0";
}
