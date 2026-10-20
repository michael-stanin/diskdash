import { lazy, Suspense, useMemo, useState } from "react";
import { GitCompareArrows, LineChart as LineChartIcon, RotateCw } from "lucide-react";
import { useLocation } from "react-router";
import { PageHeader } from "@/app/PageHeader";
import { useHistoryDiff, useHistoryVolumes, useSnapshots, useStartScan } from "@/api/queries";
import type { Snapshot } from "@/api/types/Snapshot";
import type { VolumeTrend } from "@/api/types/VolumeTrend";
import { DiffView } from "@/components/history/DiffView";
import { forecastText } from "@/components/history/trend";
import { SizeDisplay } from "@/components/SizeDisplay";
import { EmptyState, ErrorState } from "@/components/States";
import { useToast } from "@/components/Toast";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { Select } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";
import { formatAbsolute, formatBytes } from "@/lib/format";

// ECharts is large; load it only when this page draws a chart.
const FreeSpaceChart = lazy(() => import("@/components/history/FreeSpaceChart"));

function volumeName(v: VolumeTrend): string {
  const letters = v.letters.map((l) => (l.endsWith(":") ? l : `${l}:`)).join(" ");
  return v.label ? `${letters} ${v.label}`.trim() : letters || "Volume";
}

function TrendCard({ trend }: { trend: VolumeTrend }) {
  const text = forecastText(trend.forecast);
  const name = volumeName(trend);
  const days = trend.samples.length > 1 ? (trend.samples.at(-1)!.ts - trend.samples[0]!.ts) / 86_400 : 0;
  return (
    <Card className="overflow-hidden" aria-label={`Free space of ${name}`}>
      <CardHeader className="flex-wrap">
        <CardTitle>{name}</CardTitle>
        <span className="num text-xs text-muted-foreground">
          <SizeDisplay bytes={trend.capacity} /> total
        </span>
        <span
          className={cn(
            "ml-auto rounded-md px-2 py-0.5 text-[12px] font-semibold",
            text.tone === "warn" ? "bg-risk-review-bg text-risk-review-fg" : "bg-muted text-foreground-secondary",
          )}
        >
          {text.headline}
        </span>
      </CardHeader>
      <div className="flex flex-col gap-2 p-5">
        {trend.samples.length < 2 ? (
          <p className="m-0 text-[13px] text-muted-foreground">
            diskdash records free space while it runs. The chart appears after a second sample; keep it open or scan this drive.
          </p>
        ) : (
          <Suspense fallback={<Skeleton className="h-56 w-full" />}>
            <FreeSpaceChart trend={trend} label={`Free space of ${name} over the last ${Math.max(1, Math.round(days))} days`} />
          </Suspense>
        )}
        <p className="m-0 text-xs leading-relaxed text-muted-foreground">
          {text.detail} Gaps mean diskdash was not running. The dashed line extends the recent trend and is only an estimate; dots mark finished scans.
        </p>
      </div>
    </Card>
  );
}

function snapshotLabel(s: Snapshot): string {
  return [formatAbsolute(s.started_at), formatBytes(s.total_alloc), s.summarised ? "totals only" : null, s.pinned ? "pinned" : null]
    .filter(Boolean)
    .join(" · ");
}

function ComparePanel() {
  const snaps = useSnapshots();
  const scan = useStartScan();
  const { toast } = useToast();
  // A growth alert links here with ?old=<scan>&new=<scan>.
  const params = new URLSearchParams(useLocation().search);
  const linkedOld = Number(params.get("old")) || undefined;
  const linkedNew = Number(params.get("new")) || undefined;
  const [rootSel, setRoot] = useState<string>();
  const [oldSel, setOld] = useState<number | undefined>(linkedOld);
  const [newSel, setNew] = useState<number | undefined>(linkedNew);

  const byRoot = useMemo(() => {
    const m = new Map<string, Snapshot[]>();
    for (const s of snaps.data ?? []) {
      if (s.status !== "complete") continue;
      const k = s.root.toLowerCase();
      m.set(k, [...(m.get(k) ?? []), s]);
    }
    for (const list of m.values()) list.sort((a, b) => b.id - a.id);
    return m;
  }, [snaps.data]);
  // Prefer a root that already has two scans to compare.
  const roots = [...byRoot.keys()].sort((a, b) => (byRoot.get(b)!.length >= 2 ? 1 : 0) - (byRoot.get(a)!.length >= 2 ? 1 : 0));
  const linkedRoot = linkedNew ? [...byRoot.entries()].find(([, l]) => l.some((x) => x.id === linkedNew))?.[0] : undefined;
  const root = rootSel && byRoot.has(rootSel) ? rootSel : (linkedRoot ?? roots[0]);
  const list = (root ? byRoot.get(root) : undefined) ?? [];
  const newId = newSel !== undefined && list.some((s) => s.id === newSel && !s.summarised) ? newSel : list.find((s) => !s.summarised)?.id;
  const olderThanNew = list.filter((s) => newId !== undefined && s.id < newId);
  const oldId = oldSel !== undefined && olderThanNew.some((s) => s.id === oldSel) ? oldSel : olderThanNew[0]?.id;
  const diff = useHistoryDiff(oldId, newId);

  const rescan = (r: string) =>
    scan.mutate([r], { onSuccess: () => toast(`Scanning ${r}`, "info"), onError: (e) => toast(e.message, "error") });

  return (
    <section aria-label="What changed" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-3">
        <h2 className="m-0 flex items-center gap-2 text-[15px] font-semibold text-foreground-secondary">
          <GitCompareArrows className="size-4" /> What changed between scans
        </h2>
      </div>
      {snaps.isPending ? (
        <Skeleton className="h-40 w-full" />
      ) : snaps.isError ? (
        <Card>
          <ErrorState title="Could not load scans" error={snaps.error} onRetry={() => void snaps.refetch()} />
        </Card>
      ) : roots.length === 0 ? (
        <Card>
          <EmptyState icon={<GitCompareArrows />} title="No scans yet" description="Scan a drive, then scan it again later to see what changed in between." />
        </Card>
      ) : (
        <>
          <Card className="flex flex-wrap items-end gap-3 p-4">
            <label className="flex min-w-40 flex-col gap-1 text-xs text-muted-foreground">
              Folder
              <Select value={root} onChange={(e) => setRoot(e.target.value)} aria-label="Scanned folder">
                {roots.map((r) => (
                  <option key={r} value={r}>
                    {byRoot.get(r)![0]!.root}
                  </option>
                ))}
              </Select>
            </label>
            <label className="flex min-w-56 flex-1 flex-col gap-1 text-xs text-muted-foreground">
              Earlier scan
              <Select value={oldId ?? ""} onChange={(e) => setOld(Number(e.target.value))} disabled={olderThanNew.length === 0} aria-label="Earlier scan">
                {olderThanNew.map((s) => (
                  <option key={s.id} value={s.id}>
                    {snapshotLabel(s)}
                  </option>
                ))}
              </Select>
            </label>
            <label className="flex min-w-56 flex-1 flex-col gap-1 text-xs text-muted-foreground">
              Later scan
              <Select value={newId ?? ""} onChange={(e) => setNew(Number(e.target.value))} aria-label="Later scan">
                {list
                  .filter((s) => !s.summarised)
                  .map((s) => (
                    <option key={s.id} value={s.id}>
                      {snapshotLabel(s)}
                    </option>
                  ))}
              </Select>
            </label>
          </Card>
          {olderThanNew.length === 0 ? (
            <Card>
              <EmptyState
                icon={<GitCompareArrows />}
                title="Need two scans of this folder to compare"
                description="Scan it again later and diskdash shows which folders grew, shrank, appeared or disappeared."
                action={
                  <Button disabled={scan.isPending || !root} onClick={() => root && rescan(byRoot.get(root)![0]!.root)}>
                    <RotateCw className={scan.isPending ? "animate-spin" : undefined} /> Rescan now
                  </Button>
                }
              />
            </Card>
          ) : diff.isPending ? (
            <div className="flex flex-col gap-3" aria-busy="true">
              <Skeleton className="h-6 w-2/3" />
              <Skeleton className="h-48 w-full" />
            </div>
          ) : diff.isError ? (
            <Card>
              <ErrorState title="Could not compare these scans" error={diff.error} onRetry={() => void diff.refetch()} />
            </Card>
          ) : (
            <DiffView diff={diff.data} />
          )}
        </>
      )}
    </section>
  );
}

export function HistoryPage() {
  const volumes = useHistoryVolumes();
  return (
    <>
      <PageHeader
        title="History"
        subtitle="How free space changes over time, and which folders changed between scans. Older scans shrink to folder totals to save space."
      />
      <section aria-label="Free space over time" className="flex flex-col gap-3">
        <h2 className="m-0 flex items-center gap-2 text-[15px] font-semibold text-foreground-secondary">
          <LineChartIcon className="size-4" /> Free space over time
        </h2>
        {volumes.isPending ? (
          <Skeleton className="h-72 w-full rounded-card" />
        ) : volumes.isError ? (
          <Card>
            <ErrorState title="Could not load free-space history" error={volumes.error} onRetry={() => void volumes.refetch()} />
          </Card>
        ) : volumes.data.volumes.length === 0 ? (
          <Card>
            <EmptyState icon={<LineChartIcon />} title="No history yet" description="diskdash starts recording free space as soon as it sees your drives." />
          </Card>
        ) : (
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
            {volumes.data.volumes.map((v) => (
              <TrendCard key={v.volume_id} trend={v} />
            ))}
          </div>
        )}
      </section>
      <ComparePanel />
    </>
  );
}
