import type { ReactNode } from "react";
import { FilePlus2, FolderMinus, FolderPlus, TrendingDown, TrendingUp } from "lucide-react";
import type { DiffResponse } from "@/api/types/DiffResponse";
import type { DirChange } from "@/api/types/DirChange";
import { SizeDisplay } from "@/components/SizeDisplay";
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { categoryVar, CATEGORY_LABELS, type CategoryKey } from "@/lib/chart-theme";
import { cn } from "@/lib/cn";
import { formatBytes, formatCount } from "@/lib/format";

/** "+1.2 GiB" / "−300 MiB"; growth is the warning colour, shrinking the good one. */
export function Delta({ bytes, className }: { bytes: number; className?: string }) {
  if (bytes === 0) return <span className={cn("num text-muted-foreground", className)}>no change</span>;
  const grew = bytes > 0;
  return (
    <span className={cn("num font-medium", grew ? "text-fill-warning-text" : "text-success", className)}>
      {grew ? "+" : "−"}
      {formatBytes(Math.abs(bytes))}
    </span>
  );
}

function ChangeList({ title, icon, rows, empty, show }: { title: string; icon: ReactNode; rows: DirChange[]; empty: string; show: "delta" | "new" | "old" }) {
  const max = Math.max(1, ...rows.map((r) => Math.abs(show === "new" ? r.new_alloc : show === "old" ? r.old_alloc : r.delta)));
  return (
    <Card className="overflow-hidden">
      <CardHeader>
        <span className="text-muted-foreground [&_svg]:size-4">{icon}</span>
        <CardTitle>{title}</CardTitle>
      </CardHeader>
      {rows.length === 0 ? (
        <p className="m-0 px-5 py-4 text-[13px] text-muted-foreground">{empty}</p>
      ) : (
        <ul className="m-0 list-none divide-y divide-border p-0" aria-label={title}>
          {rows.map((r) => {
            const value = show === "new" ? r.new_alloc : show === "old" ? r.old_alloc : r.delta;
            return (
              <li key={r.path} className="flex flex-col gap-1 px-5 py-2.5 text-[13px]">
                <div className="flex items-baseline gap-3">
                  <span className="min-w-0 flex-1 truncate font-mono text-xs" title={r.path}>
                    {r.path}
                  </span>
                  {show === "delta" ? <Delta bytes={r.delta} /> : <SizeDisplay bytes={value} className="font-medium" />}
                </div>
                <div className="h-1 rounded-full bg-muted" aria-hidden>
                  <div
                    className={cn("h-1 rounded-full", show === "delta" ? (r.delta > 0 ? "bg-fill-warning" : "bg-fill-normal") : "bg-fill-caution")}
                    style={{ width: `${Math.max(2, (Math.abs(value) / max) * 100)}%` }}
                  />
                </div>
                {show === "delta" && (
                  <span className="num text-[11px] text-muted-foreground">
                    <SizeDisplay bytes={r.old_alloc} /> → <SizeDisplay bytes={r.new_alloc} /> · {formatCount(r.new_files)} files
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </Card>
  );
}

/** What changed between two scans of the same folder. */
export function DiffView({ diff }: { diff: DiffResponse }) {
  const maxCat = Math.max(1, ...diff.categories.map((c) => Math.abs(c.delta)));
  return (
    <div className="flex flex-col gap-4">
      <p className="m-0 flex flex-wrap items-baseline gap-x-2 text-[13px] text-muted-foreground" aria-live="polite">
        Total on disk changed by <Delta bytes={diff.total_delta} className="text-base" /> between{" "}
        <time dateTime={diff.old.started_at}>{new Date(diff.old.started_at).toLocaleDateString()}</time> and{" "}
        <time dateTime={diff.new.started_at}>{new Date(diff.new.started_at).toLocaleDateString()}</time>.
      </p>
      {diff.depth_limit !== null && (
        <p className="m-0 rounded-lg bg-muted px-3 py-2 text-xs text-muted-foreground">
          The older scan was reduced to folder totals down to level {diff.depth_limit}, so deeper folders and individual files are not compared.
        </p>
      )}
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
        <ChangeList title="Grew the most" icon={<TrendingUp />} rows={diff.growing} empty="No folder grew." show="delta" />
        <ChangeList title="Shrank the most" icon={<TrendingDown />} rows={diff.shrinking} empty="No folder shrank." show="delta" />
        <ChangeList title="New folders" icon={<FolderPlus />} rows={diff.appeared} empty="No new folders." show="new" />
        <ChangeList title="Removed folders" icon={<FolderMinus />} rows={diff.disappeared} empty="No folders were removed." show="old" />
      </div>
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card className="overflow-hidden">
          <CardHeader>
            <CardTitle>By file type</CardTitle>
          </CardHeader>
          {diff.categories.length === 0 ? (
            <p className="m-0 px-5 py-4 text-[13px] text-muted-foreground">No file type changed.</p>
          ) : (
            <ul className="m-0 list-none divide-y divide-border p-0" aria-label="Change by file type">
              {diff.categories.map((c) => (
                <li key={c.category} className="flex flex-col gap-1 px-5 py-2.5 text-[13px]">
                  <div className="flex items-center gap-2.5">
                    <span className="size-2.5 shrink-0 rounded-full" style={{ background: categoryVar(c.category) }} aria-hidden />
                    <span className="flex-1">{CATEGORY_LABELS[c.category as CategoryKey] ?? c.category}</span>
                    <Delta bytes={c.delta} />
                  </div>
                  <div className="h-1 rounded-full bg-muted" aria-hidden>
                    <div className="h-1 rounded-full" style={{ width: `${Math.max(2, (Math.abs(c.delta) / maxCat) * 100)}%`, background: categoryVar(c.category) }} />
                  </div>
                </li>
              ))}
            </ul>
          )}
        </Card>
        <Card className="overflow-hidden">
          <CardHeader>
            <span className="text-muted-foreground [&_svg]:size-4">
              <FilePlus2 />
            </span>
            <CardTitle>New large files</CardTitle>
          </CardHeader>
          {!diff.new_large_available ? (
            <p className="m-0 px-5 py-4 text-[13px] text-muted-foreground">
              Not available: the older scan was reduced to folder totals, so there is no file list to compare with.
            </p>
          ) : diff.new_large_files.length === 0 ? (
            <p className="m-0 px-5 py-4 text-[13px] text-muted-foreground">No new files of 100 MiB or more.</p>
          ) : (
            <ul className="m-0 list-none divide-y divide-border p-0" aria-label="New large files">
              {diff.new_large_files.map((f) => (
                <li key={f.path} className="flex items-baseline gap-3 px-5 py-2.5 text-[13px]">
                  <span className="min-w-0 flex-1 truncate font-mono text-xs" title={f.path}>
                    {f.path}
                  </span>
                  <SizeDisplay bytes={f.alloc} className="font-medium" />
                </li>
              ))}
            </ul>
          )}
        </Card>
      </div>
    </div>
  );
}
