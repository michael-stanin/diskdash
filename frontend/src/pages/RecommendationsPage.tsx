import { ListChecks, RotateCw } from "lucide-react";
import { Link } from "react-router";
import { PageHeader } from "@/app/PageHeader";
import { useCleanupOverview, useDrives, useRerunRules } from "@/api/queries";
import { CandidateList } from "@/components/cleanup/CandidateList";
import { riskHint, totalsOf } from "@/components/cleanup/Savings";
import { SizeDisplay } from "@/components/SizeDisplay";
import { StatCard, StatCardSkeleton } from "@/components/StatCard";
import { Button } from "@/components/ui/button";
import { RISKS } from "@/lib/rule-categories";

export function RecommendationsPage() {
  const overview = useCleanupOverview();
  const drives = useDrives();
  const rerun = useRerunRules();

  const byRisk = { Safe: 0, Review: 0, Expert: 0 };
  let system = 0;
  for (const d of overview.data?.drives ?? []) {
    const t = totalsOf(d.cleanup);
    for (const r of RISKS) byRisk[r] += t.byRisk[r];
    system += t.system;
  }
  const hasData = (overview.data?.drives.length ?? 0) > 0;

  return (
    <>
      <PageHeader
        title="Recommendations"
        subtitle="Ranked by space you could free versus risk. diskdash only suggests: you do the cleaning yourself."
        actions={
          <>
            <Button asChild variant="secondary">
              <Link to="/rules">
                <ListChecks /> Rules
              </Link>
            </Button>
            <Button variant="secondary" disabled={rerun.isPending || !hasData} onClick={() => rerun.mutate([])}>
              <RotateCw className={rerun.isPending ? "animate-spin" : undefined} /> Re-run rules
            </Button>
          </>
        }
      />
      {hasData && (
        <section aria-label="Potential savings" className="grid grid-cols-1 gap-4 sm:grid-cols-3">
          {!overview.data
            ? Array.from({ length: 3 }, (_, i) => <StatCardSkeleton key={i} />)
            : RISKS.map((r) => (
                <StatCard key={r} label={`${r} to remove`} value={<SizeDisplay bytes={byRisk[r]} variant="large" />} detail={riskHint(r)} />
              ))}
        </section>
      )}
      {hasData && system > 0 && (
        <p className="m-0 text-xs text-muted-foreground">
          Another <SizeDisplay bytes={system} /> is held by Windows settings. Those suggestions only explain what to change.
        </p>
      )}
      <CandidateList drives={drives.data?.drives} scope="your drives" />
    </>
  );
}
