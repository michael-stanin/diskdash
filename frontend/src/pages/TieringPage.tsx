import { useState } from "react";
import { AlertTriangle, ArrowRight, ChevronDown, Copy, FolderOpen, ListChecks, Settings as SettingsIcon, Sparkles } from "lucide-react";
import { Link } from "react-router";
import { PageHeader } from "@/app/PageHeader";
import { ApiError } from "@/api/client";
import { useDrives, useReveal, useSetPlanned, useTiering } from "@/api/queries";
import type { TierDrive } from "@/api/types/TierDrive";
import type { TierSuggestion } from "@/api/types/TierSuggestion";
import type { TieringResponse } from "@/api/types/TieringResponse";
import { MediaBadge } from "@/components/Badges";
import { Instructions, useCopy } from "@/components/cleanup/CandidateItem";
import { FILL_TEXT, FillBar, fillState } from "@/components/FillBar";
import { AddToPlanButton } from "@/components/plans/AddToPlan";
import { SizeDisplay } from "@/components/SizeDisplay";
import { EmptyState, ErrorState } from "@/components/States";
import { useToast } from "@/components/Toast";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";
import { formatBytes, formatPercent } from "@/lib/format";

const KIND_LABEL = { data: "Folder or file", "app-library": "App library", "app-install": "App install" } as const;

const NO_TARGET: Record<string, string> = {
  no_target_with_space: "No drive has room for it: every candidate would fall below the low-space limit plus headroom.",
  no_target_tier: "There is no drive of the other kind to move it to.",
};

function DriveBars({ drive }: { drive: TierDrive }) {
  const usedNow = drive.capacity > 0 ? 1 - drive.free / drive.capacity : 0;
  const usedAfter = drive.capacity > 0 ? 1 - drive.projected_free / drive.capacity : 0;
  const changed = drive.projected_free !== drive.free;
  const nowState = fillState(usedNow, false);
  const afterState = fillState(usedAfter, drive.low_after);
  return (
    <Card className="flex flex-col gap-3 p-4" aria-label={`Drive ${drive.letter}`}>
      <div className="flex items-center gap-2.5">
        <span className="flex size-8 items-center justify-center rounded-lg bg-muted text-[13px] font-semibold">{drive.letter}:</span>
        <MediaBadge media={drive.media} />
        <span className="num ml-auto text-xs text-muted-foreground">
          <SizeDisplay bytes={drive.capacity} /> total
        </span>
      </div>
      <div className="flex flex-col gap-1">
        <div className="flex items-baseline justify-between text-xs">
          <span className="text-muted-foreground">Now</span>
          <span className={cn("num font-medium", FILL_TEXT[nowState])}>
            <SizeDisplay bytes={drive.free} /> free
          </span>
        </div>
        <FillBar usedFraction={usedNow} state={nowState} label={`${drive.letter}: now ${formatPercent(usedNow)} used`} className="h-1.5" />
      </div>
      <div className="flex flex-col gap-1">
        <div className="flex items-baseline justify-between text-xs">
          <span className="text-muted-foreground">After planned moves</span>
          <span className={cn("num font-medium", FILL_TEXT[afterState])} aria-live="polite">
            <SizeDisplay bytes={drive.projected_free} /> free
            {changed && (
              <span className="ml-1.5 text-muted-foreground">
                ({drive.projected_free > drive.free ? "+" : "-"}
                {formatBytes(Math.abs(drive.projected_free - drive.free))})
              </span>
            )}
          </span>
        </div>
        <FillBar usedFraction={usedAfter} state={afterState} label={`${drive.letter}: after planned moves ${formatPercent(usedAfter)} used`} className="h-1.5" />
        {drive.low_after && <span className="text-xs text-fill-warning-text">{changed ? "Would be low on space" : "Already low on space"}</span>}
      </div>
    </Card>
  );
}

function Row({ s, drives }: { s: TierSuggestion; drives: TierDrive[] }) {
  const [open, setOpen] = useState(false);
  const planned = useSetPlanned();
  const reveal = useReveal();
  const copy = useCopy();
  const { toast } = useToast();
  const target = s.target ? drives.find((d) => d.id === s.target!.drive_id) : null;
  return (
    <li className="flex flex-col gap-3 px-5 py-4" aria-label={s.name}>
      <div className="flex flex-wrap items-start gap-3">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
            <h3 className="m-0 text-sm font-semibold">{s.app ?? s.name}</h3>
            <span className="rounded-md bg-muted px-2 py-0.5 text-[11px] font-semibold text-muted-foreground">{KIND_LABEL[s.kind]}</span>
          </div>
          <p className="m-0 min-w-0 truncate font-mono text-xs text-foreground-secondary" title={s.path}>
            {s.path}
          </p>
          <p className="m-0 text-[13px] text-muted-foreground">{s.reason}</p>
          {s.target ? (
            <p className="m-0 flex items-center gap-1.5 text-xs text-foreground-secondary">
              <ArrowRight className="size-3.5" /> Could go to {s.target.letter}:{target && <MediaBadge media={target.media} />}
            </p>
          ) : (
            <p className="m-0 flex items-start gap-1.5 text-xs text-fill-warning-text">
              <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
              <span>
                {NO_TARGET[s.no_target_reason ?? ""] ?? "No target drive."} <span className="font-mono">({s.no_target_reason})</span>
              </span>
            </p>
          )}
        </div>
        <span className="num text-base font-semibold" aria-label="Size on disk">
          <SizeDisplay bytes={s.allocated} />
        </span>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <label className={cn("flex items-center gap-2 text-xs", !s.target && "opacity-50")}>
          <input
            type="checkbox"
            className="accent-primary"
            checked={s.planned}
            disabled={!s.target || planned.isPending}
            onChange={(e) =>
              planned.mutate({ path: s.path, planned: e.target.checked }, { onError: (er) => toast(er instanceof ApiError ? er.message : "Could not update", "error") })
            }
          />
          Plan this move (updates the free-space bars)
        </label>
        <span className="ml-auto flex items-center gap-1.5">
          <Button
            size="icon-sm"
            variant="outline"
            aria-label={`Open ${s.name} in Explorer`}
            title="Open in Explorer"
            onClick={() =>
              reveal.mutate(
                { snapshotId: s.snapshot_id, entryId: s.entry_id },
                { onError: (e) => toast(e instanceof ApiError && e.status === 410 ? "That item no longer exists. Rescan to refresh." : e.message, "error") },
              )
            }
          >
            <FolderOpen />
          </Button>
          <Button size="icon-sm" variant="outline" aria-label={`Copy path of ${s.name}`} title="Copy path" onClick={() => copy(s.path, "Path copied")}>
            <Copy />
          </Button>
          {s.destination && (
            <AddToPlanButton what={s.path} body={{ source: "tiering", snapshot_id: s.snapshot_id, entry_id: s.entry_id, destination: s.destination }} />
          )}
          <Button size="sm" variant="secondary" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
            <ListChecks /> How to move <ChevronDown className={cn("transition-transform", open && "rotate-180")} />
          </Button>
        </span>
      </div>
      {open && (
        <div className="rounded-xl border border-border bg-muted/40 p-4">
          <Instructions lines={s.instructions} />
        </div>
      )}
    </li>
  );
}

function Section({ title, hint, items, drives }: { title: string; hint: string; items: TierSuggestion[]; drives: TierDrive[] }) {
  if (items.length === 0) return null;
  return (
    <Card className="overflow-hidden" role="region" aria-label={title}>
      <CardHeader className="flex-col items-start gap-0.5">
        <CardTitle>{title}</CardTitle>
        <p className="m-0 text-xs text-muted-foreground">{hint}</p>
      </CardHeader>
      <ul className="m-0 list-none divide-y divide-border p-0">
        {items.map((s) => (
          <Row key={s.key} s={s} drives={drives} />
        ))}
      </ul>
    </Card>
  );
}

function Empty({ data }: { data: TieringResponse }) {
  const t = data.thresholds;
  const reasons = data.notes.filter((n) => n.code !== "unscanned");
  return (
    <Card>
      <EmptyState
        icon={<Sparkles />}
        title="No tiering suggestions"
        description={
          reasons.length > 0 ? (
            <span className="flex flex-col gap-1.5">
              {reasons.map((n) => (
                <span key={n.code}>{n.message}</span>
              ))}
              <span className="text-xs">
                Thresholds: not modified for {t.cold_months} months and at least {t.min_gib} GiB (SSD to HDD); modified within {t.hot_days} days (HDD to SSD).
              </span>
            </span>
          ) : (
            `Nothing qualifies with the current thresholds: ${t.cold_months} months cold, ${t.min_gib} GiB or more, ${t.hot_days} days hot.`
          )
        }
        action={
          <Button asChild variant="secondary">
            <Link to="/settings">
              <SettingsIcon /> Adjust the thresholds
            </Link>
          </Button>
        }
      />
    </Card>
  );
}

export function TieringPage() {
  const tiering = useTiering();
  const inventory = useDrives();
  const data = tiering.data;
  // If the analysis fails, the bars still come from the drive inventory.
  const fallback = (): TierDrive[] => (inventory.data?.drives ?? []).map((d) => ({
    id: d.id,
    letter: d.letters[0] ?? "?",
    media: d.media,
    capacity: d.capacity,
    free: d.free,
    projected_free: d.free,
    low_after: d.low_space,
    scanned: false,
  }));
  const cold = data?.suggestions.filter((s) => s.direction === "cold-to-hdd") ?? [];
  const hot = data?.suggestions.filter((s) => s.direction === "hot-to-ssd") ?? [];

  return (
    <>
      <PageHeader
        title="Tiering"
        subtitle="Cold data on fast drives and busy data on slow ones. diskdash only suggests: you move things yourself, with the steps shown."
      />
      {tiering.isError ? (
        <>
          <Card>
            <ErrorState title="Cannot work out tiering suggestions" error={tiering.error} onRetry={() => tiering.refetch()} />
          </Card>
          <section aria-label="Free space" className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
            {fallback().map((d) => (
              <DriveBars key={d.id} drive={d} />
            ))}
          </section>
        </>
      ) : !data ? (
        <div className="flex flex-col gap-4" aria-label="Loading tiering">
          <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
            {Array.from({ length: 3 }, (_, i) => (
              <Skeleton key={i} className="h-40 rounded-card" />
            ))}
          </div>
          <Skeleton className="h-64 w-full rounded-card" />
        </div>
      ) : (
        <>
          <section aria-label="Free space" className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
            {data.drives.map((d) => (
              <DriveBars key={d.id} drive={d} />
            ))}
          </section>
          {data.notes.some((n) => n.code === "unscanned") && (
            <p role="note" className="m-0 text-xs text-muted-foreground">
              {data.notes.find((n) => n.code === "unscanned")!.message}
            </p>
          )}
          {data.suggestions.length === 0 ? (
            <Empty data={data} />
          ) : (
            <>
              <Section title="Cold data on SSDs" hint={`Not modified for ${data.thresholds.cold_months} months or more. Moving it to an HDD frees fast space.`} items={cold} drives={data.drives} />
              <Section title="Busy data on HDDs" hint={`Modified within ${data.thresholds.hot_days} days, and game libraries played recently. An SSD would make them faster.`} items={hot} drives={data.drives} />
            </>
          )}
          {data.manifest_errors > 0 && (
            <p className="m-0 text-xs text-warning">
              {data.manifest_errors} game launcher file{data.manifest_errors === 1 ? "" : "s"} could not be read and {data.manifest_errors === 1 ? "was" : "were"} skipped.
            </p>
          )}
          {data.as_of && <p className="m-0 text-xs text-subtle-foreground">Based on scans finished {new Date(data.as_of).toLocaleString()} or later.</p>}
        </>
      )}
    </>
  );
}
