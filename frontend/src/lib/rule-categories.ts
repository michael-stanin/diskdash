import type { Risk } from "@/api/types/Risk";
import { categoryVar } from "@/lib/chart-theme";

/** Cleanup rule categories, drawn with the shared category colour tokens. */
export const RULE_CATEGORIES: Record<string, { label: string; color: string }> = {
  windows: { label: "Windows temp & caches", color: "system" },
  upgrade: { label: "Windows upgrade leftovers", color: "disk-images" },
  system: { label: "System files", color: "other" },
  "recycle-bin": { label: "Recycle Bin", color: "archives" },
  browser: { label: "Browser caches", color: "images" },
  developer: { label: "Developer files", color: "code" },
  apps: { label: "App caches", color: "audio" },
  downloads: { label: "Downloads", color: "installers" },
  logs: { label: "Logs & temp files", color: "documents" },
};

export function ruleCategoryLabel(key: string): string {
  return RULE_CATEGORIES[key]?.label ?? key;
}

export function ruleCategoryColor(key: string): string {
  return categoryVar(RULE_CATEGORIES[key]?.color);
}

/** The API names risks `Safe`, `Review`, `Expert`; the badge component uses lower case. */
export function riskKey(risk: Risk): "safe" | "review" | "expert" {
  return risk.toLowerCase() as "safe" | "review" | "expert";
}

export const RISKS: Risk[] = ["Safe", "Review", "Expert"];
