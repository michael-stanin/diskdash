import type { Forecast } from "@/api/types/Forecast";
import type { VolumeTrend } from "@/api/types/VolumeTrend";
import type { EChartsOption } from "@/components/charts/useEChart";
import { formatBytes } from "@/lib/format";

const DAY_MS = 86_400_000;
/** No samples for this long breaks the line: a gap is not zero free space. */
export const GAP_MS = 2 * DAY_MS;
/** The forecast line is drawn at most this far ahead. */
const HORIZON_DAYS = 365;

export function formatDays(days: number): string {
  if (days < 1) return "less than a day";
  if (days < 60) return `${Math.round(days)} day${Math.round(days) === 1 ? "" : "s"}`;
  if (days < 730) return `${Math.round(days / 30)} months`;
  return `${(days / 365).toFixed(1)} years`;
}

export type ForecastTone = "ok" | "warn" | "muted";

export interface ForecastText {
  headline: string;
  detail: string;
  tone: ForecastTone;
}

/** Plain-language forecast: what we can say, and how sure we are. */
export function forecastText(f: Forecast): ForecastText {
  if (f.state === "not-enough-history") {
    return {
      headline: "Not enough history yet",
      detail: `A forecast needs at least 3 samples over 7 days; there ${f.points === 1 ? "is 1" : `are ${f.points}`} over ${f.span_days.toFixed(1)} days so far.`,
      tone: "muted",
    };
  }
  if (f.state === "not-filling") {
    return { headline: "Free space is stable", detail: `It has not been shrinking over the last ${formatDays(f.span_days)}.`, tone: "ok" };
  }
  const rough = f.low_confidence ? " This is a rough estimate: free space has gone up and down a lot." : "";
  const rate = `${formatBytes(f.bytes_per_day)} per day`;
  if (f.days_to_low <= 0) {
    return { headline: "Already below the low-space limit", detail: `Losing about ${rate}; full in about ${formatDays(f.days_to_full)}.${rough}`, tone: "warn" };
  }
  return {
    headline: `Low in about ${formatDays(f.days_to_low)}`,
    detail: `Losing about ${rate}; full in about ${formatDays(f.days_to_full)}.${rough}`,
    tone: f.days_to_low < 30 ? "warn" : "muted",
  };
}

export interface TrendColors {
  line: string;
  forecast: string;
  scan: string;
  muted: string;
  grid: string;
}

type Point = [number, number | null];

/** Free-space line with breaks at gaps, scan markers and a dashed forecast. */
export function trendSeries(trend: VolumeTrend) {
  const line: Point[] = [];
  let prev: number | null = null;
  for (const s of trend.samples) {
    const t = s.ts * 1000;
    if (prev !== null && t - prev > GAP_MS) line.push([prev + 1, null]);
    line.push([t, s.free]);
    prev = t;
  }
  const scans: Point[] = trend.samples.filter((s) => s.source === "scan").map((s) => [s.ts * 1000, s.free]);
  const last = trend.samples.at(-1);
  let forecast: Point[] = [];
  const f = trend.forecast;
  if (f.state === "filling" && last) {
    const days = Math.min(f.days_to_full, HORIZON_DAYS);
    forecast = [
      [last.ts * 1000, last.free],
      [last.ts * 1000 + days * DAY_MS, Math.max(0, last.free - f.bytes_per_day * days)],
    ];
  }
  return { line, scans, forecast };
}

export function trendOption(trend: VolumeTrend, c: TrendColors): EChartsOption {
  const { line, scans, forecast } = trendSeries(trend);
  return {
    animation: false,
    grid: { left: 64, right: 24, top: 16, bottom: 28 },
    tooltip: {
      trigger: "axis",
      valueFormatter: (v: unknown) => (typeof v === "number" ? `${formatBytes(v)} free` : "no data"),
    },
    xAxis: { type: "time", axisLine: { lineStyle: { color: c.grid } }, axisLabel: { color: c.muted } },
    yAxis: {
      type: "value",
      min: 0,
      max: trend.capacity,
      splitNumber: 4,
      axisLabel: { color: c.muted, formatter: (v: number) => formatBytes(v) },
      splitLine: { lineStyle: { color: c.grid } },
    },
    series: [
      { name: "Free space", type: "line", data: line, showSymbol: false, connectNulls: false, lineStyle: { color: c.line, width: 2 }, itemStyle: { color: c.line } },
      {
        name: "Scan finished",
        type: "line",
        data: scans,
        symbol: "circle",
        symbolSize: 8,
        lineStyle: { opacity: 0 },
        itemStyle: { color: c.scan, borderColor: c.line, borderWidth: 1.5 },
      },
      {
        name: "Forecast",
        type: "line",
        data: forecast,
        showSymbol: false,
        lineStyle: { color: c.forecast, width: 2, type: "dashed" },
        itemStyle: { color: c.forecast },
      },
    ],
  };
}
