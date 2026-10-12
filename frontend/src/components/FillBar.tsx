import { cn } from "@/lib/cn";

export type FillState = "normal" | "caution" | "warning";

/** Warning when the drive is flagged low on space, caution from 75 % used. */
export function fillState(usedFraction: number, lowSpace: boolean): FillState {
  if (lowSpace) return "warning";
  if (usedFraction >= 0.75) return "caution";
  return "normal";
}

const BAR: Record<FillState, string> = {
  normal: "bg-fill-normal",
  caution: "bg-fill-caution",
  warning: "bg-fill-warning",
};

export const FILL_TEXT: Record<FillState, string> = {
  normal: "text-fill-normal-text",
  caution: "text-fill-caution-text",
  warning: "text-fill-warning-text",
};

interface FillBarProps {
  usedFraction: number;
  state: FillState;
  label: string;
  className?: string;
}

export function FillBar({ usedFraction, state, label, className }: FillBarProps) {
  const pct = Math.max(0, Math.min(1, usedFraction)) * 100;
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(pct)}
      className={cn("h-2 overflow-hidden rounded-full bg-fill-track", className)}
    >
      <div
        className={cn("h-full rounded-full transition-[width] duration-200", BAR[state])}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}
