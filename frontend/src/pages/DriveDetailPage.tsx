import { useState, type ReactNode } from "react";
import { useParams } from "react-router";
import { HardDrive, ScanSearch } from "lucide-react";
import { PageHeader } from "@/app/PageHeader";
import { ApiError } from "@/api/client";
import { useDrive, useDrives, useSettings, useSnapshots } from "@/api/queries";
import { Button } from "@/components/ui/button";
import { Tabs } from "@/components/ui/tabs";
import { SnapshotSummary } from "@/components/usage/SnapshotSummary";
import { LazyUsageExplorer } from "@/components/usage/LazyUsageExplorer";
import { CleanupTab } from "@/components/cleanup/CleanupTab";
import { ScanActivity } from "@/components/scans/ScanActivity";
import { useScanDialog } from "@/components/scans/ScanDialog";
import { SnapshotHistory } from "@/components/scans/SnapshotHistory";
import type { HealthField } from "@/api/types/HealthField";
import type { HealthSource } from "@/api/types/HealthSource";
import type { PhysicalDisk } from "@/api/types/PhysicalDisk";
import type { TrimState } from "@/api/types/TrimState";
import { MediaBadge } from "@/components/Badges";
import { driveName, driveTitle, HealthPill, lowSpaceText } from "@/components/drives/DriveCard";
import { FillBar, fillState } from "@/components/FillBar";
import { SizeDisplay } from "@/components/SizeDisplay";
import { StatCard, StatCardSkeleton } from "@/components/StatCard";
import { EmptyState, ErrorState } from "@/components/States";
import { Card, CardBody, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { formatBytes, formatCount, formatPercent } from "@/lib/format";

const SOURCE_LABEL: Record<HealthSource, string> = {
  storage_cmdlets: "Windows storage",
  reliability_counters: "Reliability counters",
  smartctl: "smartctl",
  unavailable: "—",
};

function Row({ label, value, source }: { label: string; value: ReactNode; source?: string }) {
  return (
    <div role="row" className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)_150px] items-center gap-4 py-2.5 text-[13px]">
      <span role="rowheader" className="text-muted-foreground">
        {label}
      </span>
      <span role="cell" className="num">
        {value}
      </span>
      <span role="cell" className="text-xs text-subtle-foreground">
        {source ?? ""}
      </span>
    </div>
  );
}

function fieldValue(f: HealthField, format: (v: number) => string): ReactNode {
  if (f.value === null) {
    return <span className="text-subtle-foreground">Unavailable{f.reason ? ` — ${f.reason}` : ""}</span>;
  }
  return format(f.value);
}

function HealthRow({ label, field, format }: { label: string; field: HealthField; format: (v: number) => string }) {
  return <Row label={label} value={fieldValue(field, format)} source={SOURCE_LABEL[field.source]} />;
}

function trimRow(trim: TrimState | undefined): ReactNode {
  if (!trim || trim.ntfs === null) return <span className="text-subtle-foreground">Unavailable{trim?.reason ? ` — ${trim.reason}` : ""}</span>;
  if (trim.ntfs) return "Enabled";
  return (
    <span className="text-warning">
      Disabled. To enable, run in an admin terminal: <code className="font-mono">fsutil behavior set DisableDeleteNotify 0</code>
    </span>
  );
}

function DiskCard({ disk, trim }: { disk: PhysicalDisk; trim?: TrimState }) {
  const h = disk.health;
  const isSsd = disk.media === "NVME_SSD" || disk.media === "SATA_SSD";
  return (
    <Card>
      <CardHeader>
        <HardDrive className="size-4 text-muted-foreground" />
        <CardTitle>{disk.model}</CardTitle>
        <MediaBadge media={disk.media} />
        <span className="ml-auto">
          <HealthPill status={h.overall} reasons={h.reasons} />
        </span>
      </CardHeader>
      <CardBody className="py-2" role="table" aria-label={`${disk.model} details`}>
        <div className="divide-y divide-border">
          <Row label="Disk number" value={disk.number} />
          <Row label="Serial" value={disk.serial ?? "—"} />
          <Row label="Bus" value={disk.bus_type} />
          <Row label="Size" value={disk.size > 0 ? formatBytes(disk.size) : "—"} />
          {disk.media === "HDD" && <Row label="Spindle speed" value={disk.spindle_rpm ? `${disk.spindle_rpm} rpm` : "Not reported"} />}
          <Row
            label="Overall health"
            value={
              <>
                <HealthPill status={h.overall} />
                {h.reasons.length > 0 && <span className="ml-2 text-xs text-muted-foreground">{h.reasons.join(" · ")}</span>}
              </>
            }
            source={h.smart_passed !== null ? "Windows + SMART" : "Windows storage"}
          />
          <HealthRow label="Temperature" field={h.temperature_c} format={(v) => `${v.toFixed(0)} °C`} />
          <HealthRow label="Power-on hours" field={h.power_on_hours} format={(v) => `${formatCount(v)} h`} />
          {isSsd && <HealthRow label="Wear used" field={h.wear_percent} format={(v) => `${v.toFixed(0)}%`} />}
          {isSsd && <HealthRow label="Data written" field={h.data_written_bytes} format={(v) => formatBytes(v)} />}
          {!isSsd && <HealthRow label="Reallocated sectors" field={h.reallocated_sectors} format={(v) => formatCount(v)} />}
          <HealthRow label="Read errors" field={h.read_errors_total} format={(v) => formatCount(v)} />
          <HealthRow label="Write errors" field={h.write_errors_total} format={(v) => formatCount(v)} />
          {isSsd && <Row label="TRIM" value={trimRow(trim)} source="fsutil" />}
        </div>
      </CardBody>
    </Card>
  );
}

export function DriveDetailPage() {
  const { id = "" } = useParams();
  const drive = useDrive(id);
  const all = useDrives();
  const settings = useSettings();
  const snapshots = useSnapshots(id);
  const scanDialog = useScanDialog();
  const d = drive.data;
  const root = d?.letters[0] ? `${d.letters[0]}:\\` : d?.mount_paths[0];
  const latestComplete = snapshots.data?.find((s) => s.status === "complete" && s.root === root);
  const scanning = Boolean(snapshots.data?.some((s) => s.status === "running"));
  const [tab, setTab] = useState<"usage" | "cleanup" | "hardware" | "history">("usage");
  const scan = () => root && scanDialog.open([root]);

  if (drive.isError) {
    const notFound = drive.error instanceof ApiError && drive.error.status === 404;
    return (
      <>
        <PageHeader title="Drive" breadcrumb={[{ label: "Drives", to: "/drives" }, { label: "Not found" }]} />
        <Card>
          {notFound ? (
            <EmptyState icon={<HardDrive />} title="This drive is not connected" description="It may have been removed or reformatted." />
          ) : (
            <ErrorState title="Cannot load this drive" error={drive.error} onRetry={() => drive.refetch()} />
          )}
        </Card>
      </>
    );
  }

  const low = d && lowSpaceText(d, settings.data?.low_space_percent ?? 10, settings.data?.low_space_floor_gib ?? 20);

  return (
    <>
      <PageHeader
        title={d ? `${driveName(d)} ${driveTitle(d)}` : <Skeleton className="h-8 w-40" />}
        breadcrumb={[{ label: "Drives", to: "/drives" }, { label: d ? driveName(d) : "…" }]}
        subtitle={d ? `${d.filesystem} · ${formatBytes(d.cluster_size)} clusters${d.serial !== null ? ` · volume serial ${d.serial.toString(16).toUpperCase().padStart(8, "0")}` : ""}` : undefined}
      />
      <section aria-label="Space" className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        {!d ? (
          Array.from({ length: 3 }, (_, i) => <StatCardSkeleton key={i} />)
        ) : (
          <>
            <StatCard label="Capacity" value={<SizeDisplay bytes={d.capacity} variant="large" />} detail={<MediaBadge media={d.media} />} />
            <StatCard label="Free" value={<SizeDisplay bytes={d.free} variant="large" />} detail={low ?? `${formatPercent(1 - d.used_fraction)} free`} />
            <StatCard
              label="Used"
              value={<SizeDisplay bytes={d.used} variant="large" />}
              detail={
                <FillBar usedFraction={d.used_fraction} state={fillState(d.used_fraction, d.low_space)} label={`${formatPercent(d.used_fraction)} used`} className="mt-1" />
              }
            />
          </>
        )}
      </section>
      <ScanActivity />
      {d && (
        <Tabs
          label="Drive sections"
          value={tab}
          onChange={setTab}
          tabs={[
            { key: "usage", label: "Usage" },
            { key: "cleanup", label: "Cleanup" },
            { key: "hardware", label: "Hardware & health" },
            { key: "history", label: "Scan history" },
          ]}
        >
          {tab === "usage" &&
            (snapshots.isPending ? (
              <Skeleton className="h-[440px] w-full rounded-card" />
            ) : latestComplete ? (
              <>
                <SnapshotSummary snapshot={latestComplete} onRescan={scan} scanning={scanning} />
                <LazyUsageExplorer snapshot={latestComplete} />
              </>
            ) : (
              <Card>
                <EmptyState
                  icon={<ScanSearch />}
                  title={scanning ? "Scanning…" : "Not scanned yet"}
                  description="A scan lists every folder and file on this drive (read-only) so diskdash can show where the space goes."
                  action={
                    !scanning && (
                      <Button variant="primary" onClick={scan}>
                        <ScanSearch /> Scan now
                      </Button>
                    )
                  }
                />
              </Card>
            ))}
          {tab === "cleanup" &&
            (snapshots.isPending ? (
              <Skeleton className="h-[440px] w-full rounded-card" />
            ) : latestComplete ? (
              <CleanupTab snapshot={latestComplete} />
            ) : (
              <Card>
                <EmptyState
                  icon={<ScanSearch />}
                  title="Scan this drive to see suggestions"
                  description="Cleanup suggestions come from a scan: diskdash checks it against its rules for caches, leftovers and old files."
                  action={
                    !scanning && (
                      <Button variant="primary" onClick={scan}>
                        <ScanSearch /> Scan now
                      </Button>
                    )
                  }
                />
              </Card>
            ))}
          {tab === "hardware" && (
            <>
              {d.physical_disks.length === 0 && (
                <Card>
                  <EmptyState icon={<HardDrive />} title="Physical disk unknown" description="Windows did not report which disk this volume is on." />
                </Card>
              )}
              {d.physical_disks.map((p) => (
                <DiskCard key={p.number} disk={p} trim={all.data?.trim} />
              ))}
            </>
          )}
          {tab === "history" && <SnapshotHistory snapshots={snapshots.data} loading={snapshots.isPending} />}
        </Tabs>
      )}
    </>
  );
}
