import { useState } from "react";
import { AlertTriangle, Clock, Copy, FileText, Folder, ScanSearch } from "lucide-react";
import { useOldLarge, useStale } from "@/api/queries";
import type { BasisReason } from "@/api/types/BasisReason";
import type { Snapshot } from "@/api/types/Snapshot";
import type { StaleItem } from "@/api/types/StaleItem";
import type { StalePageView } from "@/api/types/StalePageView";
import { useCopy } from "@/components/cleanup/CandidateItem";
import { RelativeTime, SizeDisplay } from "@/components/SizeDisplay";
import { EmptyState, ErrorState } from "@/components/States";
import { EntryActions } from "@/components/usage/EntryActions";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { CATEGORY_LABELS, categoryVar, type CategoryKey } from "@/lib/chart-theme";
import { cn } from "@/lib/cn";
import { formatCount } from "@/lib/format";

export const FSUTIL_COMMAND = "fsutil behavior set disablelastaccess 0";
const PRESETS = [6, 12, 24, 36] as const;
const FLOORS = [
  { mib: 100, label: "100 MiB" },
  { mib: 500, label: "500 MiB" },
  { mib: 1024, label: "1 GiB" },
] as const;

const REASON_TEXT: Record<BasisReason, string> = {
  enabled: "Windows keeps last-access times on this drive.",
  disabled: "Last-access updates are turned off in Windows.",
  "system-disabled": "Windows manages last-access updates and has them off.",
  "system-enabled-small-volume": "Windows keeps last-access times on drives of 128 GB or less.",
  "system-enabled-large-volume": "Windows only keeps last-access times on drives of 128 GB or less, and this drive is larger.",
  unknown: "Last-access reliability unknown: the Windows setting could not be read.",
};

/** Which timestamp the list uses, and how to make last-access reliable if it is not. */
export function TimeBasisBanner({ snapshot }: { snapshot: Pick<Snapshot, "time_basis" | "time_basis_reason"> }) {
  const copy = useCopy();
  if (snapshot.time_basis === "atime") {
    return (
      <p className="m-0 flex items-center gap-2 text-xs text-muted-foreground" role="status">
        <Clock className="size-3.5" /> Using <strong className="font-semibold text-foreground-secondary">last access</strong> time. {REASON_TEXT[snapshot.time_basis_reason]}
      </p>
    );
  }
  return (
    <div role="status" className="flex flex-col gap-2 rounded-card border border-border bg-risk-review-bg px-4 py-3 text-[13px] text-risk-review-fg">
      <p className="m-0 flex items-center gap-2 font-medium">
        <AlertTriangle className="size-4" /> Using last-modified time, because last-access times are not reliable on this drive.
      </p>
      <p className="m-0">{REASON_TEXT[snapshot.time_basis_reason]} Files you open but never change can look older than they are.</p>
      <div className="flex flex-wrap items-center gap-2">
        <span>To turn last-access tracking on, run this yourself in an administrator terminal (diskdash never runs it):</span>
        <code className="rounded-md bg-card px-2 py-1 font-mono text-xs text-foreground">{FSUTIL_COMMAND}</code>
        <Button size="icon-sm" variant="outline" aria-label="Copy the fsutil command" title="Copy" onClick={() => copy(FSUTIL_COMMAND, "Command copied")}>
          <Copy />
        </Button>
      </div>
    </div>
  );
}

function Row({ item, snapshotId, showScore }: { item: StaleItem; snapshotId: number; showScore: boolean }) {
  const e = item.entry;
  const years = item.age_days >= 730 ? `${(item.age_days / 365).toFixed(1)} years` : `${formatCount(item.age_days)} days`;
  const cat = e.kind === "dir" ? e.top_category : e.category;
  return (
    <li className="flex items-center gap-3 px-5 py-3" aria-label={e.name}>
      <span className="text-muted-foreground [&_svg]:size-4">{e.kind === "dir" ? <Folder /> : <FileText />}</span>
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="min-w-0 truncate font-mono text-xs" title={e.path}>
          {e.path}
        </span>
        <span className="flex flex-wrap items-center gap-x-3 text-xs text-muted-foreground">
          {cat && (
            <span className="inline-flex items-center gap-1.5">
              <span className="size-2 rounded-full" style={{ background: categoryVar(cat) }} />
              {CATEGORY_LABELS[cat as CategoryKey] ?? cat}
            </span>
          )}
          <span>
            Last {e.kind === "dir" ? "touched inside " : ""}
            <RelativeTime date={item.time} /> ({years})
          </span>
          {e.kind === "dir" && e.file_count !== null && <span>{formatCount(e.file_count)} files</span>}
          {showScore && item.score !== null && <span title="Size times age in days">score {Math.round(item.score / 1024 ** 3).toLocaleString("en-US")} GiB·days</span>}
        </span>
      </div>
      <span className="num w-24 shrink-0 text-right text-sm font-semibold">
        <SizeDisplay bytes={e.alloc} />
      </span>
      <span className="flex shrink-0 gap-1.5">
        <EntryActions snapshotId={snapshotId} entry={e} />
      </span>
    </li>
  );
}

function SkeletonRows() {
  return (
    <ul className="m-0 list-none divide-y divide-border p-0" aria-label="Loading">
      {Array.from({ length: 5 }, (_, i) => (
        <li key={i} className="flex items-center gap-3 px-5 py-3.5" aria-hidden>
          <Skeleton className="size-4" />
          <div className="flex flex-1 flex-col gap-2">
            <Skeleton className="h-3.5 w-3/5" />
            <Skeleton className="h-3 w-2/5" />
          </div>
          <Skeleton className="h-4 w-16" />
        </li>
      ))}
    </ul>
  );
}

type Query = ReturnType<typeof useStale>;

function List({ q, snapshotId, showScore, empty }: { q: Query; snapshotId: number; showScore: boolean; empty: React.ReactNode }) {
  if (q.isError) return <ErrorState title="Cannot load this list" error={q.error} onRetry={() => q.refetch()} />;
  if (q.isPending) return <SkeletonRows />;
  const items = q.data.pages.flatMap((p) => p.data.items);
  if (items.length === 0) return <>{empty}</>;
  return (
    <>
      <ul className="m-0 list-none divide-y divide-border p-0" aria-label="Results">
        {items.map((i) => (
          <Row key={i.entry.id} item={i} snapshotId={snapshotId} showScore={showScore} />
        ))}
      </ul>
      {q.hasNextPage && (
        <div className="flex justify-center border-t border-border p-3">
          <Button variant="secondary" disabled={q.isFetchingNextPage} onClick={() => void q.fetchNextPage()}>
            {q.isFetchingNextPage ? "Loading…" : "Show more"}
          </Button>
        </div>
      )}
    </>
  );
}

function Segmented<T extends string | number>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div role="radiogroup" aria-label={label} className="flex rounded-lg border border-border-strong p-0.5">
      {options.map((o) => (
        <button
          key={String(o.value)}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onChange(o.value)}
          className={cn(
            "h-7 cursor-pointer rounded-md px-2.5 text-xs font-medium transition-colors",
            value === o.value ? "bg-primary-soft text-primary-soft-foreground" : "text-muted-foreground hover:text-foreground",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** Stale files and folders, and old large files, for one scan. */
export function StaleTab({ snapshot, defaultMonths = 12 }: { snapshot: Snapshot; defaultMonths?: number }) {
  const [view, setView] = useState<"stale" | "large">("stale");
  const [months, setMonths] = useState(defaultMonths);
  const [custom, setCustom] = useState("");
  const [floor, setFloor] = useState(100);
  const stale = useStale(snapshot.id, months);
  const large = useOldLarge(snapshot.id, floor);
  const first: StalePageView | undefined = (view === "stale" ? stale : large).data?.pages[0]?.data;
  const isPreset = (PRESETS as readonly number[]).includes(months);

  const setCustomMonths = (text: string) => {
    setCustom(text);
    const n = Number(text);
    if (Number.isInteger(n) && n >= 1 && n <= 240) setMonths(n);
  };

  return (
    <>
      <TimeBasisBanner snapshot={snapshot} />
      <Card className="overflow-hidden">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-border px-5 py-3">
          <Segmented
            label="List"
            value={view}
            options={[
              { value: "stale", label: "Not used for a while" },
              { value: "large", label: "Old large files" },
            ]}
            onChange={setView}
          />
          {view === "stale" ? (
            <>
              <Segmented
                label="Not used for"
                value={isPreset && !custom ? months : -1}
                options={PRESETS.map((m) => ({ value: m as number, label: `${m} months` }))}
                onChange={(m) => {
                  setCustom("");
                  setMonths(m);
                }}
              />
              <label className="flex items-center gap-2 text-xs text-muted-foreground">
                Other
                <Input
                  type="number"
                  min={1}
                  max={240}
                  inputMode="numeric"
                  aria-label="Custom number of months"
                  className="h-8 w-20 text-xs"
                  value={custom}
                  placeholder="months"
                  onChange={(e) => setCustomMonths(e.target.value)}
                />
              </label>
            </>
          ) : (
            <Segmented
              label="Smallest file"
              value={floor}
              options={FLOORS.map((f) => ({ value: f.mib as number, label: f.label }))}
              onChange={setFloor}
            />
          )}
          {first && first.excluded_invalid > 0 && (
            <span className="num ml-auto text-xs text-muted-foreground">{formatCount(first.excluded_invalid)} items without valid timestamps excluded</span>
          )}
        </div>
        {view === "stale" ? (
          <List
            q={stale}
            snapshotId={snapshot.id}
            showScore={false}
            empty={
              <EmptyState
                icon={<Clock />}
                title={`Nothing older than ${months} months on this drive`}
                description="Everything here was used more recently than that."
                action={
                  months > 6 && (
                    <Button variant="secondary" onClick={() => { setCustom(""); setMonths(6); }}>
                      Try 6 months
                    </Button>
                  )
                }
              />
            }
          />
        ) : (
          <List
            q={large}
            snapshotId={snapshot.id}
            showScore
            empty={
              <EmptyState
                icon={<FileText />}
                title="No old large files"
                description={`No file of at least ${FLOORS.find((f) => f.mib === floor)?.label ?? `${floor} MiB`} that is a day or more old.`}
                action={
                  floor > 100 && (
                    <Button variant="secondary" onClick={() => setFloor(100)}>
                      Lower the size to 100 MiB
                    </Button>
                  )
                }
              />
            }
          />
        )}
      </Card>
    </>
  );
}

export function StaleNeedsScan({ onScan, scanning }: { onScan: () => void; scanning: boolean }) {
  return (
    <Card>
      <EmptyState
        icon={<ScanSearch />}
        title={scanning ? "Scanning…" : "Scan this drive to find old data"}
        description="Staleness comes from the timestamps in a scan. diskdash lists what has not been used for a long time, largest first."
        action={
          !scanning && (
            <Button variant="primary" onClick={onScan}>
              <ScanSearch /> Scan now
            </Button>
          )
        }
      />
    </Card>
  );
}
