import { Settings2 } from "lucide-react";
import type { Risk } from "@/api/types/Risk";
import type { SnapshotCleanup } from "@/api/types/SnapshotCleanup";
import { SizeDisplay } from "@/components/SizeDisplay";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip } from "@/components/ui/tooltip";
import { cn } from "@/lib/cn";
import { RISKS } from "@/lib/rule-categories";

export interface SavingsTotals {
  /** Things the user can remove, per risk (allocated bytes, each counted once). */
  byRisk: Record<Risk, number>;
  /** Space controlled by Windows settings (hibernation file, shadow copies...). */
  system: number;
  /** Candidates whose size is unknown. */
  unknown: number;
}

export function totalsOf(c: SnapshotCleanup | undefined): SavingsTotals {
  const t: SavingsTotals = { byRisk: { Safe: 0, Review: 0, Expert: 0 }, system: 0, unknown: 0 };
  for (const s of c?.savings ?? []) {
    if (s.kind === "manual-cleanup") t.byRisk[s.risk] += s.allocated;
    else t.system += s.allocated;
    t.unknown += s.unknown;
  }
  return t;
}

const RISK_CLASS: Record<Risk, string> = {
  Safe: "bg-risk-safe-bg text-risk-safe-fg",
  Review: "bg-risk-review-bg text-risk-review-fg",
  Expert: "bg-risk-expert-bg text-risk-expert-fg",
};

/** Potential savings for one drive, split by risk. Compact, for drive cards. */
export function DriveSavings({ cleanup, loading }: { cleanup?: SnapshotCleanup; loading?: boolean }) {
  if (loading) return <Skeleton className="h-6 w-full" />;
  const run = cleanup?.run;
  if (!run) return null;
  if (run.status === "queued" || run.status === "running") {
    return <p className="m-0 text-xs text-muted-foreground">Looking for things to clean up…</p>;
  }
  if (run.status === "failed") {
    return <p className="m-0 text-xs text-warning">Could not check this scan for savings{run.error ? ` (${run.error})` : ""}.</p>;
  }
  const t = totalsOf(cleanup);
  const manual = t.byRisk.Safe + t.byRisk.Review + t.byRisk.Expert;
  if (manual === 0 && t.system === 0) {
    return <p className="m-0 text-xs text-muted-foreground">Nothing to clean up found in the last scan.</p>;
  }
  return (
    <div className="flex flex-col gap-1.5" aria-label="Potential savings">
      <div className="flex flex-wrap items-center gap-1.5 text-xs">
        <span className="text-muted-foreground">{manual > 0 ? "Could free" : "Nothing to remove by hand"}</span>
        {RISKS.filter((r) => t.byRisk[r] > 0).map((r) => (
          <Tooltip key={r} content={`${r}: ${riskHint(r)}`}>
            <span
              tabIndex={0}
              className={cn("num inline-flex items-center gap-1 rounded-md px-2 py-0.5 font-semibold", RISK_CLASS[r])}
            >
              {r} <SizeDisplay bytes={t.byRisk[r]} />
            </span>
          </Tooltip>
        ))}
      </div>
      {t.system > 0 && (
        <p className="m-0 flex items-center gap-1.5 text-xs text-muted-foreground">
          <Settings2 className="size-3" />
          <span>
            Plus <SizeDisplay bytes={t.system} /> held by Windows settings
          </span>
        </p>
      )}
    </div>
  );
}

export function riskHint(r: Risk): string {
  return r === "Safe" ? "rebuilt automatically" : r === "Review" ? "check before removing" : "only if you know the impact";
}
