import { Link } from "react-router";
import { ChevronRight } from "lucide-react";
import type { Drive } from "@/api/types/Drive";
import type { HealthStatus } from "@/api/types/HealthStatus";
import type { SnapshotCleanup } from "@/api/types/SnapshotCleanup";
import { MediaBadge } from "@/components/Badges";
import { DriveSavings } from "@/components/cleanup/Savings";
import { FILL_TEXT, FillBar, fillState } from "@/components/FillBar";
import { RelativeTime, SizeDisplay } from "@/components/SizeDisplay";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip } from "@/components/ui/tooltip";
import { cn } from "@/lib/cn";
import { formatBytesParts, formatPercent } from "@/lib/format";

const HEALTH: Record<HealthStatus, { label: string; dot: string }> = {
  healthy: { label: "Healthy", dot: "bg-success" },
  warning: { label: "Warning", dot: "bg-warning" },
  unhealthy: { label: "Unhealthy", dot: "bg-danger" },
  unknown: { label: "Health unknown", dot: "bg-subtle-foreground" },
};

export function HealthPill({ status, reasons, limited }: { status: HealthStatus; reasons?: string[]; limited?: boolean }) {
  const h = HEALTH[status];
  const tip = [
    ...(reasons ?? []),
    ...(limited ? ["Limited: detailed SMART counters need administrator rights"] : []),
  ];
  const pill = (
    <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground" tabIndex={tip.length ? 0 : -1}>
      <span className={cn("size-[7px] rounded-full", h.dot)} />
      {h.label}
      {limited && <span className="text-subtle-foreground">· limited (not elevated)</span>}
    </span>
  );
  return tip.length > 0 ? <Tooltip content={tip.join(" · ")}>{pill}</Tooltip> : pill;
}

export function driveName(d: Drive): string {
  return d.letters.length > 0 ? `${d.letters[0]}:` : (d.mount_paths[0] ?? "Volume");
}

export function driveTitle(d: Drive): string {
  if (d.label) return d.label;
  if (d.is_system_volume) return "Windows";
  return "Local disk";
}

export function lowSpaceText(d: Drive, percent: number, floorGib: number): string | null {
  switch (d.low_space_reason) {
    case "percent":
      return `Below ${percent}% free`;
    case "floor":
      return `Below ${floorGib} GiB free`;
    case "both":
      return `Below ${percent}% and ${floorGib} GiB free`;
    default:
      return null;
  }
}

interface DriveCardProps {
  drive: Drive;
  thresholds: { percent: number; floorGib: number };
  /** Finish time of the latest complete whole-drive scan. */
  lastScan?: string | null;
  /** Health from basic Windows status only (not elevated). */
  limitedHealth?: boolean;
  /** Cleanup suggestions from the latest whole-drive scan. */
  savings?: SnapshotCleanup;
}

export function DriveCard({ drive: d, thresholds, lastScan, limitedHealth, savings }: DriveCardProps) {
  const state = fillState(d.used_fraction, d.low_space);
  const free = formatBytesParts(d.free);
  const model = d.physical_disks.map((p) => p.model).join(" + ") || "Physical disk unknown";
  const low = lowSpaceText(d, thresholds.percent, thresholds.floorGib);

  return (
    <Card className="flex flex-col gap-4 p-5" aria-label={`Drive ${driveName(d)}`}>
      <div className="flex items-center gap-3">
        <div className="flex size-10 shrink-0 items-center justify-center rounded-[10px] bg-muted text-[15px] font-semibold">
          {driveName(d).slice(0, 2)}
        </div>
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="truncate font-semibold">{driveTitle(d)}</span>
          <span className="truncate text-xs text-muted-foreground" title={model}>
            {model}
          </span>
        </div>
        <MediaBadge media={d.media} className="ml-auto" />
      </div>

      <div className="flex items-baseline gap-2">
        <span className="num text-[30px] leading-none font-semibold tracking-tight">{free.value}</span>
        <span className="text-muted-foreground">{free.unit} free of</span>
        <SizeDisplay bytes={d.capacity} className="text-foreground-secondary" />
        <span className={cn("num ml-auto text-[13px] font-medium", FILL_TEXT[state])}>
          {formatPercent(d.used_fraction)} used
        </span>
      </div>

      <FillBar usedFraction={d.used_fraction} state={state} label={`${driveName(d)} ${formatPercent(d.used_fraction)} used`} />

      <DriveSavings cleanup={savings} />

      <div className="flex flex-wrap items-center gap-x-3.5 gap-y-1.5 text-xs text-muted-foreground">
        <HealthPill status={d.health} reasons={d.physical_disks.flatMap((p) => p.health.reasons)} limited={limitedHealth} />
        <span>{d.filesystem || "—"}</span>
        {low && <span className="font-medium text-fill-warning-text">{low}</span>}
        {lastScan ? (
          <span>
            Scanned <RelativeTime date={lastScan} />
          </span>
        ) : (
          !low && <span>Not scanned yet</span>
        )}
        <Link
          to={`/drives/${d.id}`}
          className="ml-auto inline-flex items-center gap-0.5 rounded text-[13px] font-medium text-primary no-underline hover:text-primary-hover"
          aria-label={`Details for ${driveName(d)}`}
        >
          Details <ChevronRight className="size-3.5" />
        </Link>
      </div>
    </Card>
  );
}

export function DriveCardSkeleton() {
  return (
    <Card className="flex flex-col gap-4 p-5" aria-hidden>
      <div className="flex items-center gap-3">
        <Skeleton className="size-10 rounded-[10px]" />
        <div className="flex flex-col gap-2">
          <Skeleton className="h-4 w-24" />
          <Skeleton className="h-3 w-36" />
        </div>
        <Skeleton className="ml-auto h-5 w-12" />
      </div>
      <Skeleton className="h-8 w-40" />
      <Skeleton className="h-2 w-full rounded-full" />
      <Skeleton className="h-3 w-48" />
    </Card>
  );
}
