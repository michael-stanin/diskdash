import { TrendingDown } from "lucide-react";
import type { Forecast } from "@/api/types/Forecast";
import { cn } from "@/lib/cn";
import { forecastText, type ForecastTone } from "./trend";

const TONE: Record<ForecastTone, string> = {
  ok: "text-muted-foreground",
  muted: "text-muted-foreground",
  warn: "font-medium text-fill-warning-text",
};

/** One line on a drive card: what the free-space history says. */
export function ForecastBadge({ forecast }: { forecast: Forecast }) {
  const t = forecastText(forecast);
  return (
    <p className={cn("m-0 flex items-center gap-1.5 text-xs", TONE[t.tone])} title={t.detail}>
      <TrendingDown className="size-3.5 shrink-0" aria-hidden />
      <span>Forecast: {t.headline}</span>
    </p>
  );
}
