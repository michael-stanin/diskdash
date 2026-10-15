import { useEffect, useMemo, useState } from "react";
import { ScanSearch, Search, Sparkles } from "lucide-react";
import { useCandidates, useRerunRules, type CandidateFilter } from "@/api/queries";
import type { Drive } from "@/api/types/Drive";
import type { Risk } from "@/api/types/Risk";
import type { RuleKind } from "@/api/types/RuleKind";
import { CandidateItem, CandidateItemSkeleton } from "@/components/cleanup/CandidateItem";
import { driveName } from "@/components/drives/DriveCard";
import { useScanDialog } from "@/components/scans/ScanDialog";
import { SizeDisplay } from "@/components/SizeDisplay";
import { EmptyState, ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Select } from "@/components/ui/input";
import { cn } from "@/lib/cn";
import { formatCount } from "@/lib/format";
import { RISKS, RULE_CATEGORIES } from "@/lib/rule-categories";

interface CandidateListProps {
  /** One scan (drive tab). Omit to cover the latest scan of every drive. */
  snapshotId?: number;
  /** Drives for the drive filter (Recommendations page). */
  drives?: Drive[];
  /** Name used in empty states: "this drive" or "your drives". */
  scope?: string;
}

function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

export function CandidateList({ snapshotId, drives, scope = "your drives" }: CandidateListProps) {
  const [risk, setRisk] = useState<Risk | null>(null);
  const [category, setCategory] = useState("");
  const [kind, setKind] = useState<RuleKind | "">("");
  const [drive, setDrive] = useState("");
  const [text, setText] = useState("");
  const q = useDebounced(text, 250);
  const scanDialog = useScanDialog();
  const rerun = useRerunRules();

  const filter: CandidateFilter = useMemo(
    () => ({ snapshotId, drive: drive || undefined, risk, category: category || null, kind: kind || null, q }),
    [snapshotId, drive, risk, category, kind, q],
  );
  const result = useCandidates(filter);
  const pages = result.data?.pages ?? [];
  const first = pages[0];
  const items = pages.flatMap((p) => p.items);
  const filtered = Boolean(risk || category || kind || drive || q.trim());
  const clear = () => {
    setRisk(null);
    setCategory("");
    setKind("");
    setDrive("");
    setText("");
  };

  let body;
  if (result.isError) {
    body = <ErrorState title="Cannot load suggestions" error={result.error} onRetry={() => result.refetch()} />;
  } else if (result.isPending) {
    body = (
      <ul className="m-0 list-none divide-y divide-border p-0" aria-label="Loading suggestions">
        {Array.from({ length: 4 }, (_, i) => (
          <CandidateItemSkeleton key={i} />
        ))}
      </ul>
    );
  } else if (first && first.scanned === 0 && !snapshotId) {
    body = (
      <EmptyState
        icon={<ScanSearch />}
        title="Scan a drive to see suggestions"
        description="diskdash checks a scan against its cleanup rules to find caches, leftovers and old files you could remove. Nothing is deleted for you."
        action={
          <Button variant="primary" onClick={() => scanDialog.open()}>
            <ScanSearch /> Scan a drive
          </Button>
        }
      />
    );
  } else if (items.length === 0 && first && first.evaluating > 0) {
    body = (
      <>
        <p role="status" className="m-0 px-5 pt-4 text-[13px] text-muted-foreground">
          Checking the scan against the cleanup rules…
        </p>
        <ul className="m-0 list-none divide-y divide-border p-0" aria-label="Loading suggestions">
          {Array.from({ length: 3 }, (_, i) => (
            <CandidateItemSkeleton key={i} />
          ))}
        </ul>
      </>
    );
  } else if (items.length === 0 && first && first.failed > 0) {
    body = (
      <ErrorState
        title="Could not check this scan against the rules"
        error={first.error ?? "The evaluation failed."}
        onRetry={() => rerun.mutate(snapshotId ? [snapshotId] : [])}
      />
    );
  } else if (items.length === 0) {
    body = filtered ? (
      <EmptyState
        icon={<Search />}
        title="Nothing matches these filters"
        action={
          <Button variant="secondary" onClick={clear}>
            Clear filters
          </Button>
        }
      />
    ) : (
      <EmptyState
        icon={<Sparkles />}
        title={`Nothing to suggest for ${scope}`}
        description="None of the cleanup rules found anything worth removing in the latest scan. That is good news."
      />
    );
  } else {
    body = (
      <>
        <ul className="m-0 list-none divide-y divide-border p-0" aria-label="Suggestions">
          {items.map((c) => (
            <CandidateItem key={`${c.snapshot_id}-${c.match_id}`} item={c} showDrive={!snapshotId} />
          ))}
        </ul>
        {result.hasNextPage && (
          <div className="flex justify-center border-t border-border p-3">
            <Button variant="secondary" disabled={result.isFetchingNextPage} onClick={() => void result.fetchNextPage()}>
              {result.isFetchingNextPage ? "Loading…" : "Show more"}
            </Button>
          </div>
        )}
      </>
    );
  }

  return (
    <Card className="overflow-hidden">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-border px-5 py-3">
        <div role="radiogroup" aria-label="Risk" className="flex rounded-lg border border-border-strong p-0.5">
          {([null, ...RISKS] as (Risk | null)[]).map((r) => (
            <button
              key={r ?? "all"}
              type="button"
              role="radio"
              aria-checked={risk === r}
              onClick={() => setRisk(r)}
              className={cn(
                "h-7 cursor-pointer rounded-md px-2.5 text-xs font-medium transition-colors",
                risk === r ? "bg-primary-soft text-primary-soft-foreground" : "text-muted-foreground hover:text-foreground",
              )}
            >
              {r ?? "All risks"}
            </button>
          ))}
        </div>
        {drives && drives.length > 1 && (
          <Select aria-label="Drive" className="h-8 w-auto text-xs" value={drive} onChange={(e) => setDrive(e.target.value)}>
            <option value="">All drives</option>
            {drives.map((d) => (
              <option key={d.id} value={d.id}>
                {driveName(d)}
              </option>
            ))}
          </Select>
        )}
        <Select aria-label="Category" className="h-8 w-auto text-xs" value={category} onChange={(e) => setCategory(e.target.value)}>
          <option value="">All categories</option>
          {Object.entries(RULE_CATEGORIES).map(([key, c]) => (
            <option key={key} value={key}>
              {c.label}
            </option>
          ))}
        </Select>
        <Select aria-label="Type" className="h-8 w-auto text-xs" value={kind} onChange={(e) => setKind(e.target.value as RuleKind | "")}>
          <option value="">Everything</option>
          <option value="manual-cleanup">Things to remove</option>
          <option value="system-setting">Windows settings</option>
        </Select>
        <label className="flex h-8 w-52 items-center gap-2 rounded-lg border border-border-strong px-2.5 text-muted-foreground focus-within:border-primary">
          <Search className="size-3.5 shrink-0" />
          <input
            type="search"
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="Search paths"
            aria-label="Search paths"
            className="w-full bg-transparent text-xs text-foreground outline-none placeholder:text-subtle-foreground"
          />
        </label>
        {first && first.total > 0 && (
          <span className="num ml-auto text-xs text-muted-foreground" aria-live="polite">
            {formatCount(first.total)} suggestion{first.total === 1 ? "" : "s"} · <SizeDisplay bytes={first.total_allocated} />
          </span>
        )}
      </div>
      {first && first.failed > 0 && items.length > 0 && (
        <p className="m-0 border-b border-border bg-risk-review-bg px-5 py-2 text-xs text-risk-review-fg">
          One scan could not be checked ({first.error}). Results below are from the other scans.
        </p>
      )}
      {body}
    </Card>
  );
}
