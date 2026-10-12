import { AlertTriangle, HardDrive, HeartPulse, RotateCw, ScanSearch } from "lucide-react";
import { PageHeader } from "@/app/PageHeader";
import { useDrives, useLatestSnapshots, useRefreshDrives, useSettings } from "@/api/queries";
import { ScanActivity } from "@/components/scans/ScanActivity";
import { useScanDialog } from "@/components/scans/ScanDialog";
import { DriveCard, DriveCardSkeleton } from "@/components/drives/DriveCard";
import { excludedNote } from "@/components/drives/excluded";
import { RelativeTime, SizeDisplay } from "@/components/SizeDisplay";
import { StatCard, StatCardSkeleton } from "@/components/StatCard";
import { EmptyState, ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { formatPercent } from "@/lib/format";

const MEDIA_LABEL = { NVME_SSD: "NVMe", SATA_SSD: "SATA SSD", HDD: "HDD", UNKNOWN: "Unknown" } as const;

export function HomePage() {
  const drives = useDrives();
  const settings = useSettings();
  const refresh = useRefreshDrives();
  const latest = useLatestSnapshots();
  const scanDialog = useScanDialog();
  const lastScanFor = (id: string, letter?: string) =>
    latest.data?.find((s) => s.volume_id === id && s.root === `${letter}:\\`)?.finished_at ?? null;
  const thresholds = {
    percent: settings.data?.low_space_percent ?? 10,
    floorGib: settings.data?.low_space_floor_gib ?? 20,
  };

  const data = drives.data;
  const list = data?.drives ?? [];
  const capacity = list.reduce((s, d) => s + d.capacity, 0);
  const free = list.reduce((s, d) => s + d.free, 0);
  const low = list.filter((d) => d.low_space).length;
  const unhealthy = list.filter((d) => d.health === "warning" || d.health === "unhealthy").length;
  const note = data ? excludedNote(data) : null;

  return (
    <>
      <PageHeader
        title="Overview"
        subtitle={
          data?.refreshed_at ? (
            <>
              {list.length} drive{list.length === 1 ? "" : "s"} · updated <RelativeTime date={data.refreshed_at} /> ·
              read-only, nothing is changed on disk
            </>
          ) : (
            "Read-only: diskdash shows and suggests, it never changes anything on disk."
          )
        }
        actions={
          <>
            <Button variant="secondary" onClick={() => refresh.mutate()} disabled={refresh.isPending}>
              <RotateCw className={refresh.isPending ? "animate-spin" : undefined} /> Refresh
            </Button>
            <Button variant="primary" onClick={() => scanDialog.open()}>
              <ScanSearch /> Scan drives
            </Button>
          </>
        }
      />

      <ScanActivity />

      {drives.isError ? (
        <Card>
          <ErrorState title="Cannot read your drives" error={drives.error} onRetry={() => drives.refetch()} />
        </Card>
      ) : (
        <>
          <section aria-label="Summary" className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
            {!data ? (
              Array.from({ length: 4 }, (_, i) => <StatCardSkeleton key={i} />)
            ) : (
              <>
                <StatCard
                  label="Total capacity"
                  value={<SizeDisplay bytes={capacity} variant="large" />}
                  detail={[...new Set(list.map((d) => MEDIA_LABEL[d.media]))].join(" · ")}
                />
                <StatCard
                  label="Free space"
                  value={<SizeDisplay bytes={free} variant="large" />}
                  detail={capacity > 0 ? `${formatPercent(free / capacity)} of all drives` : undefined}
                />
                <StatCard
                  label="Drives low on space"
                  icon={low > 0 ? <AlertTriangle /> : undefined}
                  value={
                    <>
                      {low} <span className="text-base font-medium text-muted-foreground">of {list.length}</span>
                    </>
                  }
                  detail={`Below ${thresholds.percent}% or ${thresholds.floorGib} GiB free`}
                />
                <StatCard
                  label="Disk health"
                  icon={<HeartPulse />}
                  value={unhealthy === 0 ? "Good" : `${unhealthy} warning${unhealthy === 1 ? "" : "s"}`}
                  detail={data.sources.elevated ? "From Windows and SMART counters" : "Basic status (admin shows more)"}
                />
              </>
            )}
          </section>

          <section aria-label="Drives" className="flex flex-col gap-3">
            <h2 className="m-0 text-[15px] font-semibold text-foreground-secondary">Drives</h2>
            {!data ? (
              <div className="grid grid-cols-1 gap-4 lg:grid-cols-2 xl:grid-cols-3">
                {Array.from({ length: 3 }, (_, i) => (
                  <DriveCardSkeleton key={i} />
                ))}
              </div>
            ) : list.length === 0 ? (
              <Card>
                <EmptyState
                  icon={<HardDrive />}
                  title="No internal drives found"
                  description={data.error ?? "diskdash could not find any fixed internal drive. Try Refresh."}
                />
              </Card>
            ) : (
              <div className="grid grid-cols-1 gap-4 lg:grid-cols-2 xl:grid-cols-3">
                {list.map((d) => (
                  <DriveCard
                    key={d.id}
                    drive={d}
                    thresholds={thresholds}
                    lastScan={lastScanFor(d.id, d.letters[0])}
                    limitedHealth={!data.sources.elevated}
                  />
                ))}
              </div>
            )}
            {note && <p className="m-0 text-xs text-subtle-foreground">{note}</p>}
            {data?.stale && (
              <p className="m-0 text-xs text-warning">
                The last refresh failed ({data.error}); showing earlier values.
              </p>
            )}
          </section>
        </>
      )}
    </>
  );
}
