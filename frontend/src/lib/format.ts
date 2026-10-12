const UNITS = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"] as const;

/**
 * Human-readable size in binary units with at most three significant figures,
 * trailing zeros trimmed: 123456789012 → "115 GiB", 1536 → "1.5 KiB".
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  // Rounding can carry into the next unit (1023.9 KiB would print as "1024 KiB").
  if (value >= 1023.5 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${trimZeros(value)} ${UNITS[unit]}`;
}

/** Split form for big-number displays: { value: "115", unit: "GiB" }. */
export function formatBytesParts(bytes: number): { value: string; unit: string } {
  const [value = "—", unit = ""] = formatBytes(bytes).split(" ");
  return { value, unit };
}

function trimZeros(value: number): string {
  const decimals = value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return value.toFixed(decimals).replace(/\.?0+$/, "");
}

/** Exact count for tooltips: "123,456,789,012 bytes". */
export function formatExactBytes(bytes: number): string {
  return `${Math.round(bytes).toLocaleString("en-US")} bytes`;
}

export function formatCount(n: number): string {
  return Math.round(n).toLocaleString("en-US");
}

export function formatPercent(fraction: number, digits = 0): string {
  if (!Number.isFinite(fraction)) return "—";
  return `${(fraction * 100).toFixed(digits)}%`;
}

const RTF = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
const STEPS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 3600],
  ["month", 30 * 24 * 3600],
  ["week", 7 * 24 * 3600],
  ["day", 24 * 3600],
  ["hour", 3600],
  ["minute", 60],
];

/** "3 months ago", "yesterday", "just now". */
export function formatRelative(date: Date | string, now: Date = new Date()): string {
  const d = typeof date === "string" ? new Date(date) : date;
  const seconds = (d.getTime() - now.getTime()) / 1000;
  if (!Number.isFinite(seconds)) return "—";
  for (const [unit, size] of STEPS) {
    if (Math.abs(seconds) >= size) return RTF.format(Math.round(seconds / size), unit);
  }
  return "just now";
}

export function formatAbsolute(date: Date | string): string {
  const d = typeof date === "string" ? new Date(date) : date;
  return d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** "6 min 12 s", "45 s", "1 h 3 min". */
export function formatDuration(totalSeconds: number): string {
  const s = Math.max(0, Math.round(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h > 0) return m > 0 ? `${h} h ${m} min` : `${h} h`;
  if (m > 0) return sec > 0 ? `${m} min ${sec} s` : `${m} min`;
  return `${sec} s`;
}
