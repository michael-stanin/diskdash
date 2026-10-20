import { useMemo } from "react";
import type { VolumeTrend } from "@/api/types/VolumeTrend";
import { useEChart } from "@/components/charts/useEChart";
import { readToken } from "@/lib/chart-theme";
import { useTheme } from "@/lib/theme";
import { trendOption } from "./trend";

/** Free space over time with scan markers and a dashed forecast (loaded lazily, with ECharts). */
export default function FreeSpaceChart({ trend, label }: { trend: VolumeTrend; label: string }) {
  const { resolved } = useTheme();
  const option = useMemo(
    () => {
      void resolved; // colours are read from the active theme's tokens
      return trendOption(trend, {
        line: readToken("primary"),
        forecast: readToken("fill-warning"),
        scan: readToken("card"),
        muted: readToken("muted-foreground"),
        grid: readToken("border"),
      });
    },
    [trend, resolved],
  );
  const ref = useEChart(option);
  return <div ref={ref} role="img" aria-label={label} className="h-56 w-full" />;
}
