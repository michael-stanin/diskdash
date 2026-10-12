/**
 * ECharts theme built from the CSS design tokens at runtime, so charts follow
 * the light/dark theme without any colour values in code.
 */

export const CATEGORY_KEYS = [
  "video",
  "images",
  "audio",
  "archives",
  "installers",
  "documents",
  "code",
  "disk-images",
  "system",
  "other",
] as const;
export type CategoryKey = (typeof CATEGORY_KEYS)[number];

export const CATEGORY_LABELS: Record<CategoryKey, string> = {
  video: "Video",
  images: "Images",
  audio: "Audio",
  archives: "Archives",
  installers: "Installers & ISOs",
  documents: "Documents",
  code: "Code & dev",
  "disk-images": "Disk images",
  system: "System",
  other: "Other",
};

export function readToken(name: string, el: Element = document.documentElement): string {
  return getComputedStyle(el).getPropertyValue(`--${name}`).trim();
}

export function categoryColor(key: CategoryKey | string | null | undefined): string {
  const k = key && (CATEGORY_KEYS as readonly string[]).includes(key) ? key : "other";
  return readToken(`cat-${k}`);
}

/** CSS colour for a category (for HTML elements; follows the theme). */
export function categoryVar(key: string | null | undefined): string {
  const k = key && (CATEGORY_KEYS as readonly string[]).includes(key) ? key : "other";
  return `var(--cat-${k})`;
}

/** Theme object for `echarts.registerTheme` / `init(el, theme)`. */
export function buildChartTheme(): Record<string, unknown> {
  const text = readToken("foreground");
  const muted = readToken("muted-foreground");
  const border = readToken("border");
  const card = readToken("card-raised");
  const font = getComputedStyle(document.body).fontFamily;
  return {
    color: CATEGORY_KEYS.map(categoryColor),
    backgroundColor: "transparent",
    textStyle: { color: text, fontFamily: font },
    title: { textStyle: { color: text }, subtextStyle: { color: muted } },
    legend: { textStyle: { color: muted } },
    tooltip: {
      backgroundColor: card,
      borderColor: border,
      textStyle: { color: text, fontFamily: font },
      extraCssText: "box-shadow: var(--shadow-popover); border-radius: 10px;",
    },
    categoryAxis: {
      axisLine: { lineStyle: { color: border } },
      axisLabel: { color: muted },
      splitLine: { lineStyle: { color: border } },
    },
    valueAxis: {
      axisLine: { lineStyle: { color: border } },
      axisLabel: { color: muted },
      splitLine: { lineStyle: { color: border } },
    },
    timeAxis: { axisLabel: { color: muted }, splitLine: { lineStyle: { color: border } } },
  };
}
