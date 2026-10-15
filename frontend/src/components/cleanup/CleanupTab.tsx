import { AlertTriangle, RotateCw } from "lucide-react";
import { Link } from "react-router";
import { useRerunRules, useSnapshotCleanup } from "@/api/queries";
import type { Snapshot } from "@/api/types/Snapshot";
import { CandidateList } from "@/components/cleanup/CandidateList";
import { riskHint, totalsOf } from "@/components/cleanup/Savings";
import { SizeDisplay } from "@/components/SizeDisplay";
import { StatCard, StatCardSkeleton } from "@/components/StatCard";
import { ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { RISKS } from "@/lib/rule-categories";

/** Cleanup suggestions for one scan, shown in the drive view. */
export function CleanupTab({ snapshot }: { snapshot: Snapshot }) {
  const cleanup = useSnapshotCleanup(snapshot.id);
  const rerun = useRerunRules();
  const run = cleanup.data?.run;
  const totals = totalsOf(cleanup.data);

  if (cleanup.isError) {
    return (
      <Card>
        <ErrorState title="Cannot load the suggestions" error={cleanup.error} onRetry={() => cleanup.refetch()} />
      </Card>
    );
  }
  if (run?.status === "failed") {
    return (
      <Card>
        <ErrorState
          title="Could not check this scan against the cleanup rules"
          error={run.error ?? "The evaluation failed."}
          onRetry={() => rerun.mutate([snapshot.id])}
        />
      </Card>
    );
  }
  return (
    <>
      {run?.stale && (
        <div role="status" className="flex flex-wrap items-center gap-3 rounded-card border border-border bg-risk-review-bg px-4 py-3 text-[13px] text-risk-review-fg">
          <AlertTriangle className="size-4" />
          <span>The cleanup rules changed since this scan was checked.</span>
          <Button size="sm" variant="secondary" className="ml-auto" disabled={rerun.isPending} onClick={() => rerun.mutate([snapshot.id])}>
            <RotateCw className={rerun.isPending ? "animate-spin" : undefined} /> Re-run rules
          </Button>
        </div>
      )}
      {run && run.rule_errors > 0 && (
        <p className="m-0 text-xs text-warning">
          {run.rule_errors} rule problem{run.rule_errors === 1 ? "" : "s"} were skipped.{" "}
          <Link to="/rules" className="text-primary no-underline hover:text-primary-hover">
            See the Rules page
          </Link>
        </p>
      )}
      <section aria-label="Potential savings" className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        {!cleanup.data || run?.status === "queued" || run?.status === "running"
          ? Array.from({ length: 3 }, (_, i) => <StatCardSkeleton key={i} />)
          : RISKS.map((r) => (
              <StatCard
                key={r}
                label={`${r} to remove`}
                value={<SizeDisplay bytes={totals.byRisk[r]} variant="large" />}
                detail={riskHint(r)}
              />
            ))}
      </section>
      {totals.system > 0 && (
        <p className="m-0 text-xs text-muted-foreground">
          Another <SizeDisplay bytes={totals.system} /> is held by Windows settings (hibernation file, shadow copies and similar). Those are listed below with the steps to change them.
        </p>
      )}
      {!cleanup.data ? <Skeleton className="h-40 w-full rounded-card" /> : <CandidateList snapshotId={snapshot.id} scope="this drive" />}
    </>
  );
}
