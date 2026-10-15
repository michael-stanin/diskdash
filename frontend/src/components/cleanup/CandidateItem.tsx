import { useState } from "react";
import { ChevronDown, Copy, FolderOpen, Layers, ListChecks, Settings2 } from "lucide-react";
import { ApiError } from "@/api/client";
import { useNestedCandidates, useReveal } from "@/api/queries";
import type { CandidateView } from "@/api/types/CandidateView";
import { RiskBadge } from "@/components/Badges";
import { AddToPlanButton } from "@/components/plans/AddToPlan";
import { isCommand } from "@/components/cleanup/instructions";
import { RelativeTime, SizeDisplay } from "@/components/SizeDisplay";
import { useToast } from "@/components/Toast";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/cn";
import { formatCount } from "@/lib/format";
import { riskKey, ruleCategoryColor, ruleCategoryLabel } from "@/lib/rule-categories";

export function useCopy() {
  const { toast } = useToast();
  return (text: string, what = "Copied") => {
    void navigator.clipboard?.writeText(text).then(
      () => toast(what),
      () => toast("Could not copy to the clipboard", "error"),
    );
  };
}

/** The steps for the user, with copy buttons. Nothing here is ever run by diskdash. */
export function Instructions({ lines }: { lines: string[] }) {
  const copy = useCopy();
  return (
    <div className="flex flex-col gap-2">
      <ol className="m-0 flex list-none flex-col gap-1.5 p-0">
        {lines.map((line, i) =>
          isCommand(line) ? (
            <li key={i} className="flex items-center gap-2">
              <code className="min-w-0 flex-1 overflow-x-auto rounded-md bg-muted px-2.5 py-1.5 font-mono text-xs whitespace-pre text-foreground">
                {line}
              </code>
              <Button size="icon-sm" variant="outline" aria-label={`Copy command ${line}`} title="Copy" onClick={() => copy(line, "Command copied")}>
                <Copy />
              </Button>
            </li>
          ) : (
            <li key={i} className="text-[13px] leading-relaxed text-foreground-secondary">
              {line}
            </li>
          ),
        )}
      </ol>
      <p className="m-0 text-xs text-subtle-foreground">diskdash only shows these steps. You run them yourself.</p>
    </div>
  );
}

function NestedList({ snapshotId, parent }: { snapshotId: number; parent: number }) {
  const nested = useNestedCandidates(snapshotId, parent, true);
  if (nested.isPending) return <p className="m-0 text-xs text-muted-foreground">Loading…</p>;
  if (nested.isError) return <p className="m-0 text-xs text-danger">Could not load the smaller matches.</p>;
  return (
    <ul className="m-0 flex list-none flex-col gap-1 p-0" aria-label="Included items">
      {nested.data.items.map((n) => (
        <li key={n.match_id} className="flex items-baseline gap-3 text-xs">
          <span className="min-w-0 flex-1 truncate font-mono text-muted-foreground" title={n.path}>
            {n.path}
          </span>
          <span className="shrink-0 text-subtle-foreground">{n.title}</span>
          <span className="num w-20 shrink-0 text-right text-muted-foreground">
            {n.allocated === null ? "—" : <SizeDisplay bytes={n.allocated} />}
          </span>
        </li>
      ))}
    </ul>
  );
}

interface CandidateItemProps {
  item: CandidateView;
  /** Show the drive the item lives on (when the list spans drives). */
  showDrive?: boolean;
}

export function CandidateItem({ item, showDrive }: CandidateItemProps) {
  const [open, setOpen] = useState(false);
  const reveal = useReveal();
  const { toast } = useToast();
  const copy = useCopy();
  const system = item.kind === "system-setting";
  const panelId = `candidate-${item.snapshot_id}-${item.match_id}`;

  const openInExplorer = () => {
    if (item.entry_id === null) return;
    reveal.mutate(
      { snapshotId: item.snapshot_id, entryId: item.entry_id },
      {
        onError: (e) =>
          toast(e instanceof ApiError && e.status === 410 ? "That item no longer exists — rescan to refresh" : e.message, "error"),
      },
    );
  };

  return (
    <li className="flex flex-col gap-3 px-5 py-4" aria-label={item.title}>
      <div className="flex items-start gap-3">
        <span className="mt-1.5 size-2.5 shrink-0 rounded-full" style={{ background: ruleCategoryColor(item.category) }} title={ruleCategoryLabel(item.category)} />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
            <h3 className="m-0 text-sm font-semibold">{item.title}</h3>
            <RiskBadge risk={riskKey(item.risk)} />
            {system && (
              <span className="inline-flex items-center gap-1 rounded-md bg-muted px-2 py-0.5 text-[11px] font-semibold text-muted-foreground" title="Controlled by a Windows setting; you follow the steps">
                <Settings2 className="size-3" /> Windows setting
              </span>
            )}
            <span className="text-xs text-subtle-foreground">{ruleCategoryLabel(item.category)}</span>
            {showDrive && <span className="font-mono text-xs text-subtle-foreground">{item.root.slice(0, 2)}</span>}
          </div>
          <p className="m-0 text-[13px] leading-relaxed text-muted-foreground">{item.explanation}</p>
          <p className="m-0 min-w-0 truncate font-mono text-xs text-foreground-secondary" title={item.path}>
            {item.path}
          </p>
          <p className="m-0 flex flex-wrap gap-x-3 text-xs text-subtle-foreground">
            {item.file_count !== null && item.is_dir && <span>{formatCount(item.file_count)} files</span>}
            {item.modified && (
              <span>
                Changed <RelativeTime date={item.modified} />
              </span>
            )}
            {item.nested > 0 && (
              <span className="inline-flex items-center gap-1">
                <Layers className="size-3" /> includes {item.nested} smaller match{item.nested === 1 ? "" : "es"}
              </span>
            )}
            {item.note && <span className={item.allocated === null ? "text-fill-warning-text" : undefined}>{item.note}</span>}
          </p>
        </div>
        <div className="flex shrink-0 flex-col items-end gap-2">
          <span className="num text-base font-semibold" aria-label="Size on disk">
            {item.allocated === null ? <span className="text-sm font-medium text-muted-foreground">Size unknown</span> : <SizeDisplay bytes={item.allocated} />}
          </span>
          <div className="flex items-center gap-1.5">
            {item.entry_id !== null && (
              <Button size="icon-sm" variant="outline" aria-label={`Open ${item.title} in Explorer`} title="Open in Explorer" onClick={openInExplorer}>
                <FolderOpen />
              </Button>
            )}
            <Button size="icon-sm" variant="outline" aria-label={`Copy path of ${item.title}`} title="Copy path" onClick={() => copy(item.path, "Path copied")}>
              <Copy />
            </Button>
            <AddToPlanButton what={item.path} size="icon-sm" body={{ source: "cleanup", snapshot_id: item.snapshot_id, match_id: item.match_id }} />
            <Button size="sm" variant="secondary" aria-expanded={open} aria-controls={panelId} onClick={() => setOpen((o) => !o)}>
              <ListChecks /> How to
              <ChevronDown className={cn("transition-transform", open && "rotate-180")} />
            </Button>
          </div>
        </div>
      </div>
      {open && (
        <div id={panelId} className="ml-[22px] flex flex-col gap-3 rounded-xl border border-border bg-muted/40 p-4">
          <Instructions lines={item.instructions} />
          {item.nested > 0 && (
            <div className="flex flex-col gap-2 border-t border-border pt-3">
              <h4 className="m-0 text-xs font-semibold text-muted-foreground">Also matched inside (not counted again)</h4>
              <NestedList snapshotId={item.snapshot_id} parent={item.match_id} />
            </div>
          )}
        </div>
      )}
    </li>
  );
}

export function CandidateItemSkeleton() {
  return (
    <li className="flex items-start gap-3 px-5 py-4" aria-hidden>
      <div className="mt-1.5 size-2.5 rounded-full bg-muted" />
      <div className="flex flex-1 flex-col gap-2">
        <div className="h-4 w-56 animate-shimmer rounded-md bg-muted" />
        <div className="h-3 w-4/5 animate-shimmer rounded-md bg-muted" />
        <div className="h-3 w-2/5 animate-shimmer rounded-md bg-muted" />
      </div>
      <div className="h-5 w-16 animate-shimmer rounded-md bg-muted" />
    </li>
  );
}
