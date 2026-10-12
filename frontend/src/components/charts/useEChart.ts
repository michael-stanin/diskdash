import { useEffect, useRef } from "react";
import { SunburstChart, TreemapChart } from "echarts/charts";
import { TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { buildChartTheme } from "@/lib/chart-theme";
import { useTheme } from "@/lib/theme";

// Only what diskdash draws: keeps the bundle small.
echarts.use([TreemapChart, SunburstChart, TooltipComponent, CanvasRenderer]);

export type EChartsOption = echarts.EChartsCoreOption;
export type ClickParams = { data?: unknown; dataIndex?: number };

export function prefersReducedMotion(): boolean {
  return typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
}

/**
 * Mount an ECharts instance on `ref`, themed from the design tokens. The chart
 * is rebuilt when the light/dark theme changes, resized with its container,
 * and disposed on unmount.
 */
export function useEChart(option: EChartsOption | null, onClick?: (p: ClickParams) => void) {
  const ref = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);
  const clickRef = useRef(onClick);
  const { resolved } = useTheme();

  useEffect(() => {
    clickRef.current = onClick;
  }, [onClick]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const instance = echarts.init(el, buildChartTheme(), { renderer: "canvas" });
    chart.current = instance;
    instance.on("click", (p) => clickRef.current?.(p as ClickParams));
    const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(() => instance.resize()) : null;
    ro?.observe(el);
    return () => {
      ro?.disconnect();
      instance.dispose();
      chart.current = null;
    };
    // `resolved` re-creates the chart so the theme follows light/dark.
  }, [resolved]);

  useEffect(() => {
    if (chart.current && option) {
      chart.current.setOption(option, { notMerge: true, lazyUpdate: false });
    }
  }, [option, resolved]);

  return ref;
}
