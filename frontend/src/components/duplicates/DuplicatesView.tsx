import { useMemo, useState } from "react";
import { AlertTriangle, ChevronDown, Copy, FolderOpen, Loader2, Search, Sparkles, Square } from "lucide-react";
import { ApiError } from "@/api/client";
import { useEvents } from "@/api/events";
import {
  useCancelJob,
  useClearSuggestions,
  useDupGroups,
  useDupNear,
  useDupSkips,
  useDupStatus,
  useLatestSnapshots,
  useReveal,
  useStartDuplicates,
  useSuggestKeep,
  type DupFilter,
} from "@/api/queries";
import type { DupGroup } from "@/api/types/DupGroup";
import type { DupMember } from "@/api/types/DupMember";
import type { DupRun } from "@/api/types/DupRun";
import { useCopy } from "@/components/cleanup/CandidateItem";
import { FillBar } from "@/components/FillBar";
import { RelativeTime, SizeDisplay } from "@/components/SizeDisplay";
import { EmptyState, ErrorState } from "@/components/States";
import { useToast } from "@/components/Toast";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input, Select } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { CATEGORY_LABELS, categoryVar, type CategoryKey } from "@/lib/chart-theme";
import { cn } from "@/lib/cn";
import { formatBytes, formatCount, formatDuration } from "@/lib/format";

const SKIP_LABEL: Record<string, string> = {
  "cloud-placeholder": "Online-only cloud files (never opened)",
  "reparse-point": "Links and junctions",
  "hard-link": "Other names of the same file",
  protected: "Inside Windows folders",
  "changed-since-scan": "Changed since the scan",
  "sharing-violation": "In use by another program",
  "access-denied": "Access denied",
  "not-found": "No longer there",
  error: "Could not be read",
};

const RULES = [
  { value: "newest", label: "Keep the newest" },
  { value: "oldest", label: "Keep the oldest" },
  { value: "shortest-path", label: "Keep the shortest path" },
  { value: "prefer-drive", label: "Prefer a drive" },
  { value: "prefer-folder", label: "Prefer a folder" },
] as const;

export const REMOVE_STEPS = "Open it in Explorer, check it, then delete it to the Recycle Bin yourself.";

function Atime({ roots }: { roots: string[] }) {
  if (roots.length === 0) return null;
  return (
    <p role="note" className="m-0 flex items-start gap-2 text-xs text-muted-foreground">
      <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
      <span>
        Comparing files means reading them. On {roots.map((r) => r.slice(0, 2)).join(", ")}, where Windows keeps last-access times,
        that can update those times. Nothing else about your files changes.
      </span>
    </p>
  );
}

function StartPanel({ snapshotIds, rerun }: { snapshotIds?: number[]; rerun?: DupRun }) {
  const start = useStartDuplicates();
  const latest = useLatestSnapshots();
  const [minMib, setMinMib] = useState(1);
  const [near, setNear] = useState(false);
  const scope = (latest.data ?? []).filter((s) => (snapshotIds ? snapshotIds.includes(s.id) : /^[A-Za-z]:\\$/.test(s.root)));
  const atime = scope.filter((s) => s.time_basis === "atime").map((s) => s.root);
  const hasScan = scope.length > 0;
  const error = start.error instanceof ApiError ? start.error.message : start.error ? "Could not start" : null;
  return (
    <div className="flex flex-col items-center gap-4">
      <div className="flex flex-wrap items-center justify-center gap-4 text-[13px]">
        <label className="flex items-center gap-2 text-muted-foreground">
          Smallest file
          <Select aria-label="Smallest file" className="h-8 w-auto text-xs" value={minMib} onChange={(e) => setMinMib(Number(e.target.value))}>
            <option value={1}>1 MiB</option>
            <option value={10}>10 MiB</option>
            <option value={100}>100 MiB</option>
          </Select>
        </label>
        <label className="flex items-center gap-2 text-muted-foreground">
          <input type="checkbox" className="accent-primary" checked={near} onChange={(e) => setNear(e.target.checked)} />
          Also find near-duplicate folders
        </label>
      </div>
      <Atime roots={atime} />
      <Button
        variant="primary"
        disabled={start.isPending || !hasScan}
        onClick={() => start.mutate({ snapshot_ids: snapshotIds, min_size_mib: minMib, near_duplicates: near })}
      >
        <Search /> {rerun ? "Search again" : "Find duplicates"}
      </Button>
      {!hasScan && <p className="m-0 text-xs text-muted-foreground">Scan a drive first: duplicates are found from a finished scan.</p>}
      {error && <p role="alert" className="m-0 text-xs text-danger">{error}</p>}
    </div>
  );
}

function Progress({ run }: { run: DupRun }) {
  const { activeJobs } = useEvents();
  const cancel = useCancelJob();
  const job = activeJobs.find((j) => j.id === run.job_id);
  const fraction = job?.total ? Math.min(1, job.done / job.total) : null;
  return (
    <Card className="flex flex-col gap-2.5 px-5 py-4" aria-label="Duplicate search in progress">
      <div className="flex items-center gap-3">
        <Loader2 className="size-4 animate-spin text-primary" />
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{job?.phase ?? "Starting"}</span>
        <span className="num text-xs text-muted-foreground">
          {job ? formatBytes(job.done) : "0 B"}
          {job?.total ? ` of ${formatBytes(job.total)}` : ""}
          {job?.eta_seconds != null ? ` · about ${formatDuration(job.eta_seconds)} left` : ""}
        </span>
        <Button size="sm" variant="secondary" disabled={cancel.isPending || run.job_id === null} onClick={() => run.job_id !== null && cancel.mutate(run.job_id)} aria-label="Cancel the duplicate search">
          <Square /> Cancel
        </Button>
      </div>
      {fraction !== null ? (
        <FillBar usedFraction={fraction} state="normal" label="Bytes read" className="h-1.5" />
      ) : (
        <div className="h-1.5 overflow-hidden rounded-full bg-fill-track">
          <div className="h-full w-1/3 animate-shimmer rounded-full bg-fill-normal" />
        </div>
      )}
      <Atime roots={run.atime_roots} />
    </Card>
  );
}

function MemberRow({ m }: { m: DupMember }) {
  const reveal = useReveal();
  const copy = useCopy();
  const { toast } = useToast();
  return (
    <li className="flex flex-col gap-1 px-4 py-2.5">
      <div className="flex items-center gap-3">
        {m.suggested && (
          <span
            className={cn(
              "w-16 shrink-0 rounded-md px-2 py-0.5 text-center text-[11px] font-semibold",
              m.suggested === "keep" ? "bg-risk-safe-bg text-risk-safe-fg" : "bg-risk-review-bg text-risk-review-fg",
            )}
          >
            {m.suggested === "keep" ? "Keep" : "Remove"}
          </span>
        )}
        <span className="min-w-0 flex-1 truncate font-mono text-xs" title={m.path}>
          {m.path}
        </span>
        {m.modified && (
          <span className="shrink-0 text-xs text-muted-foreground">
            <RelativeTime date={m.modified} />
          </span>
        )}
        <Button
          size="icon-sm"
          variant="outline"
          aria-label={`Open ${m.path} in Explorer`}
          title="Open in Explorer"
          onClick={() =>
            reveal.mutate(
              { snapshotId: m.snapshot_id, entryId: m.entry_id },
              { onError: (e) => toast(e instanceof ApiError && e.status === 410 ? "That file no longer exists. Search again to refresh." : e.message, "error") },
            )
          }
        >
          <FolderOpen />
        </Button>
        <Button size="icon-sm" variant="outline" aria-label={`Copy path ${m.path}`} title="Copy path" onClick={() => copy(m.path, "Path copied")}>
          <Copy />
        </Button>
      </div>
      {m.suggested === "remove" && <p className="m-0 pl-[76px] text-xs text-muted-foreground">{REMOVE_STEPS}</p>}
    </li>
  );
}

function GroupItem({ g }: { g: DupGroup }) {
  const [open, setOpen] = useState(false);
  const cat = g.members[0]?.category ?? null;
  const name = g.members[0]?.path.split("\\").pop() ?? "";
  return (
    <li className="flex flex-col" aria-label={`Duplicates of ${name}`}>
      <button type="button" aria-expanded={open} onClick={() => setOpen((o) => !o)} className="flex cursor-pointer items-center gap-3 px-5 py-3.5 text-left hover:bg-hover/50">
        <span className="size-2.5 shrink-0 rounded-full" style={{ background: categoryVar(cat) }} title={cat ? (CATEGORY_LABELS[cat as CategoryKey] ?? cat) : undefined} />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-sm font-semibold">{name}</span>
          <span className="text-xs text-muted-foreground">
            {g.members.length} copies of <SizeDisplay bytes={g.size} />
            {g.rule && <span className="ml-2 rounded-md bg-muted px-1.5 py-0.5 text-[11px]">suggestion: {g.rule}</span>}
          </span>
        </span>
        <span className="num text-right">
          <span className="block text-sm font-semibold">
            <SizeDisplay bytes={g.wasted} />
          </span>
          <span className="block text-xs text-muted-foreground">wasted</span>
        </span>
        <ChevronDown className={cn("size-4 text-muted-foreground transition-transform", open && "rotate-180")} />
      </button>
      {open && (
        <ul className="m-0 list-none divide-y divide-border border-t border-border bg-muted/30 p-0" aria-label="Copies">
          {g.members.map((m) => (
            <MemberRow key={m.id} m={m} />
          ))}
        </ul>
      )}
    </li>
  );
}

function Skips({ run }: { run: DupRun }) {
  const [reason, setReason] = useState<string | null>(null);
  const skips = useDupSkips(reason);
  const entries = Object.entries(run.skips).sort((a, b) => b[1] - a[1]);
  if (entries.length === 0) return null;
  return (
    <details className="text-xs text-muted-foreground">
      <summary className="cursor-pointer select-none">Left out: {formatCount(entries.reduce((s, [, n]) => s + n, 0))} files</summary>
      <ul className="m-0 mt-2 flex list-none flex-col gap-1 p-0">
        {entries.map(([key, n]) => (
          <li key={key} className="flex items-center gap-2">
            <span className="num w-14 text-right">{formatCount(n)}</span>
            <span>{SKIP_LABEL[key] ?? key}</span>
            {["changed-since-scan", "sharing-violation", "access-denied", "not-found", "error"].includes(key) && (
              <Button size="sm" variant="ghost" onClick={() => setReason(reason === key ? null : key)}>
                {reason === key ? "Hide" : "Show files"}
              </Button>
            )}
          </li>
        ))}
      </ul>
      {reason && (
        <ul className="m-0 mt-2 max-h-40 list-none overflow-auto rounded-md bg-muted p-2 font-mono" aria-label="Skipped files">
          {(skips.data ?? []).map((s) => (
            <li key={s.path} className="truncate" title={s.path}>
              {s.path}
            </li>
          ))}
        </ul>
      )}
    </details>
  );
}

function NearList({ run }: { run: DupRun }) {
  const near = useDupNear(run.near_enabled);
  if (!run.near_enabled) return null;
  return (
    <Card className="overflow-hidden" aria-label="Near-duplicate folders">
      <h3 className="m-0 border-b border-border px-5 py-3 text-sm font-semibold">Near-duplicate folders</h3>
      {near.isPending ? (
        <Skeleton className="m-5 h-10" />
      ) : (near.data ?? []).length === 0 ? (
        <p className="m-0 px-5 py-4 text-[13px] text-muted-foreground">No folders are similar enough (90% or more, with at least 10 files).</p>
      ) : (
        <ul className="m-0 list-none divide-y divide-border p-0">
          {near.data!.map((p) => (
            <li key={`${p.dir_a}|${p.dir_b}`} className="flex flex-col gap-0.5 px-5 py-3 text-[13px]">
              <span className="font-mono text-xs">{p.dir_a}</span>
              <span className="font-mono text-xs">{p.dir_b}</span>
              <span className="text-xs text-muted-foreground">
                {Math.round(p.similarity * 100)}% the same · {formatCount(p.shared_files)} shared files (<SizeDisplay bytes={p.shared_bytes} />) · <SizeDisplay bytes={p.differing_bytes} /> differs
              </span>
            </li>
          ))}
        </ul>
      )}
      {run.near_truncated && <p className="m-0 border-t border-border px-5 py-2 text-xs text-warning">More pairs exist than are shown (limit reached).</p>}
    </Card>
  );
}

interface Props {
  /** One scan (drive tab); omit for every drive. */
  snapshotId?: number;
  /** Drive letter filter for the groups shown. */
  drive?: string;
  /** Drives for the drive filter (page). */
  drives?: { letter: string }[];
}

/** Duplicate search: start, progress, results, keep-rule suggestions. */
export function DuplicatesView({ snapshotId, drive: fixedDrive, drives }: Props) {
  const status = useDupStatus();
  const [driveF, setDriveF] = useState("");
  const [folder, setFolder] = useState("");
  const [category, setCategory] = useState("");
  const [sort, setSort] = useState<DupFilter["sort"]>("wasted");
  const filter: DupFilter = useMemo(() => ({ drive: fixedDrive ?? driveF, folder: folder.trim(), category, sort }), [fixedDrive, driveF, folder, category, sort]);
  const run = status.data ?? undefined;
  const active = run?.status === "queued" || run?.status === "running";
  const groups = useDupGroups(filter, Boolean(run) && !active);
  const suggest = useSuggestKeep();
  const clear = useClearSuggestions();
  const [rule, setRule] = useState<string>("newest");
  const [value, setValue] = useState("");
  const { toast } = useToast();
  const items = groups.data?.pages.flatMap((p) => p.groups) ?? [];
  const first = groups.data?.pages[0];
  const needsValue = rule === "prefer-drive" || rule === "prefer-folder";
  const snapshotIds = snapshotId !== undefined ? [snapshotId] : undefined;

  if (status.isError) return <Card><ErrorState title="Cannot load the duplicate search" error={status.error} onRetry={() => status.refetch()} /></Card>;
  if (status.isPending) {
    return (
      <Card className="flex flex-col gap-3 p-5" aria-label="Loading duplicates">
        <Skeleton className="h-6 w-1/3" />
        <Skeleton className="h-16 w-full" />
        <Skeleton className="h-16 w-full" />
      </Card>
    );
  }
  if (!run) {
    return (
      <Card>
        <EmptyState
          icon={<Search />}
          title="Find duplicate files"
          description="diskdash compares files of the same size by reading them (first the ends, then the whole file where needed) to find exact copies. It only reads: nothing is changed, moved or deleted."
          action={<StartPanel snapshotIds={snapshotIds} />}
        />
      </Card>
    );
  }
  if (active) return <Progress run={run} />;

  return (
    <>
      {(run.status === "failed" || run.status === "cancelled") && (
        <Card>
          <ErrorState title={run.status === "cancelled" ? "The search was cancelled" : "The duplicate search failed"} error={run.error ?? "No reason was given."} />
          <div className="flex justify-center pb-8">
            <StartPanel snapshotIds={snapshotIds} rerun={run} />
          </div>
        </Card>
      )}
      {run.status === "complete" && (
        <>
          <Card className="flex flex-wrap items-center gap-x-6 gap-y-2 px-5 py-4" aria-label="Last search">
            <div className="flex flex-col">
              <span className="num text-xl font-semibold">
                <SizeDisplay bytes={run.wasted} />
              </span>
              <span className="text-xs text-muted-foreground">wasted by {formatCount(run.group_count)} duplicate group{run.group_count === 1 ? "" : "s"}</span>
            </div>
            <div className="flex min-w-0 flex-1 flex-col text-xs text-muted-foreground">
              <span>
                Searched {run.roots.join(", ")} · files of {formatBytes(run.min_size)} or more · {formatCount(run.candidates)} compared ({run.algo})
              </span>
              <span>
                {run.finished_at && <RelativeTime date={run.finished_at} />} · {formatCount(run.cache_hits)} reused from earlier searches
              </span>
            </div>
            <StartPanel snapshotIds={snapshotIds} rerun={run} />
          </Card>
          <Skips run={run} />
          <Card className="overflow-hidden">
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-border px-5 py-3">
              {!fixedDrive && drives && drives.length > 1 && (
                <Select aria-label="Drive" className="h-8 w-auto text-xs" value={driveF} onChange={(e) => setDriveF(e.target.value)}>
                  <option value="">All drives</option>
                  {drives.map((d) => (
                    <option key={d.letter} value={d.letter}>
                      {d.letter}:
                    </option>
                  ))}
                </Select>
              )}
              <Select aria-label="Type" className="h-8 w-auto text-xs" value={category} onChange={(e) => setCategory(e.target.value)}>
                <option value="">All types</option>
                {Object.entries(CATEGORY_LABELS).map(([k, label]) => (
                  <option key={k} value={k}>
                    {label}
                  </option>
                ))}
              </Select>
              <Input aria-label="Folder" placeholder="Folder (path)" className="h-8 w-56 text-xs" value={folder} onChange={(e) => setFolder(e.target.value)} />
              <Select aria-label="Sort" className="h-8 w-auto text-xs" value={sort} onChange={(e) => setSort(e.target.value as DupFilter["sort"])}>
                <option value="wasted">Most wasted space</option>
                <option value="size">Largest files</option>
                <option value="count">Most copies</option>
              </Select>
              {first && first.total > 0 && (
                <span className="num ml-auto text-xs text-muted-foreground" aria-live="polite">
                  {formatCount(first.total)} group{first.total === 1 ? "" : "s"} · <SizeDisplay bytes={first.total_wasted} /> wasted
                </span>
              )}
            </div>
            {first && first.total > 0 && (
              <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-border bg-muted/30 px-5 py-3 text-[13px]">
                <Sparkles className="size-4 text-muted-foreground" />
                <Select aria-label="Keep rule" className="h-8 w-auto text-xs" value={rule} onChange={(e) => setRule(e.target.value)}>
                  {RULES.map((r) => (
                    <option key={r.value} value={r.value}>
                      {r.label}
                    </option>
                  ))}
                </Select>
                {needsValue && (
                  <Input
                    aria-label={rule === "prefer-drive" ? "Drive letter" : "Folder to prefer"}
                    placeholder={rule === "prefer-drive" ? "D" : "D:\\Photos"}
                    className="h-8 w-44 text-xs"
                    value={value}
                    onChange={(e) => setValue(e.target.value)}
                  />
                )}
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={suggest.isPending || (needsValue && !value.trim())}
                  onClick={() =>
                    suggest.mutate(
                      { rule, value: value.trim() || undefined, drive: filter.drive || undefined, folder: filter.folder || undefined, category: filter.category || undefined },
                      { onSuccess: (r) => toast(`Suggested which copy to keep in ${r.applied} group${r.applied === 1 ? "" : "s"}`), onError: (e) => toast(e.message, "error") },
                    )
                  }
                >
                  Suggest which to keep
                </Button>
                <Button size="sm" variant="ghost" disabled={clear.isPending} onClick={() => clear.mutate()}>
                  Clear suggestions
                </Button>
                <span className="text-xs text-muted-foreground">Suggestions only. You remove copies yourself.</span>
              </div>
            )}
            {groups.isError ? (
              <ErrorState title="Cannot load the groups" error={groups.error} onRetry={() => groups.refetch()} />
            ) : groups.isPending ? (
              <ul className="m-0 list-none divide-y divide-border p-0" aria-label="Loading groups">
                {Array.from({ length: 4 }, (_, i) => (
                  <li key={i} className="flex items-center gap-3 px-5 py-4" aria-hidden>
                    <Skeleton className="size-2.5 rounded-full" />
                    <div className="flex flex-1 flex-col gap-2">
                      <Skeleton className="h-4 w-1/3" />
                      <Skeleton className="h-3 w-1/4" />
                    </div>
                    <Skeleton className="h-5 w-16" />
                  </li>
                ))}
              </ul>
            ) : items.length === 0 ? (
              <EmptyState
                icon={<Sparkles />}
                title={filter.drive || filter.folder || filter.category ? "No duplicates match these filters" : `No duplicates over ${formatBytes(run.min_size)} found`}
                description={`Searched ${run.roots.join(", ")}${run.finished_at ? `, ${new Date(run.finished_at).toLocaleDateString()}` : ""}.`}
              />
            ) : (
              <>
                <ul className="m-0 list-none divide-y divide-border p-0" aria-label="Duplicate groups">
                  {items.map((g) => (
                    <GroupItem key={g.id} g={g} />
                  ))}
                </ul>
                {groups.hasNextPage && (
                  <div className="flex justify-center border-t border-border p-3">
                    <Button variant="secondary" disabled={groups.isFetchingNextPage} onClick={() => void groups.fetchNextPage()}>
                      {groups.isFetchingNextPage ? "Loading…" : "Show more"}
                    </Button>
                  </div>
                )}
              </>
            )}
          </Card>
          <NearList run={run} />
        </>
      )}
    </>
  );
}
