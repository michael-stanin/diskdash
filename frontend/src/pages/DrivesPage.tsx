import { HardDrive, RotateCw } from "lucide-react";
import { PageHeader } from "@/app/PageHeader";
import { useCleanupOverview, useDrives, useLatestSnapshots, useRefreshDrives, useSettings } from "@/api/queries";
import { ScanActivity } from "@/components/scans/ScanActivity";
import { DriveCard, DriveCardSkeleton } from "@/components/drives/DriveCard";
import { excludedNote } from "@/components/drives/excluded";
import { RelativeTime } from "@/components/SizeDisplay";
import { EmptyState, ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";

export function DrivesPage() {
  const drives = useDrives();
  const settings = useSettings();
  const refresh = useRefreshDrives();
  const latest = useLatestSnapshots();
  const cleanup = useCleanupOverview();
  const thresholds = {
    percent: settings.data?.low_space_percent ?? 10,
    floorGib: settings.data?.low_space_floor_gib ?? 20,
  };
  const data = drives.data;
  const note = data ? excludedNote(data) : null;

  return (
    <>
      <PageHeader
        title="Drives"
        subtitle={
          data?.refreshed_at ? (
            <>
              Fixed internal drives · updated <RelativeTime date={data.refreshed_at} />
            </>
          ) : (
            "Every fixed internal drive, with its hardware and health."
          )
        }
        actions={
          <Button variant="secondary" onClick={() => refresh.mutate()} disabled={refresh.isPending}>
            <RotateCw className={refresh.isPending ? "animate-spin" : undefined} /> Refresh
          </Button>
        }
      />
      <ScanActivity />
      {drives.isError ? (
        <Card>
          <ErrorState title="Cannot read your drives" error={drives.error} onRetry={() => drives.refetch()} />
        </Card>
      ) : !data ? (
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2 xl:grid-cols-3">
          {Array.from({ length: 3 }, (_, i) => (
            <DriveCardSkeleton key={i} />
          ))}
        </div>
      ) : data.drives.length === 0 ? (
        <Card>
          <EmptyState icon={<HardDrive />} title="No internal drives found" description={data.error ?? undefined} />
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2 xl:grid-cols-3">
          {data.drives.map((d) => (
            <DriveCard
              key={d.id}
              drive={d}
              thresholds={thresholds}
              lastScan={latest.data?.find((s) => s.volume_id === d.id && s.root === `${d.letters[0]}:\\`)?.finished_at}
              limitedHealth={!data.sources.elevated}
              savings={cleanup.data?.drives.find((x) => x.volume_id === d.id && x.root === `${d.letters[0]}:\\`)?.cleanup}
            />
          ))}
        </div>
      )}
      {note && <p className="m-0 text-xs text-subtle-foreground">{note}</p>}
      {data?.fallback && (
        <p className="m-0 text-xs text-warning">
          PowerShell was unavailable, so drive details come from a basic Windows query (no model names or health).
        </p>
      )}
    </>
  );
}
