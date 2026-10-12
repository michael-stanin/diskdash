import { useState } from "react";
import { PieChart } from "lucide-react";
import { useCategoriesAt, useExtensionsAt } from "@/api/queries";
import { SizeDisplay } from "@/components/SizeDisplay";
import { EmptyState, ErrorState } from "@/components/States";
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { CATEGORY_LABELS, categoryVar, type CategoryKey } from "@/lib/chart-theme";
import { cn } from "@/lib/cn";
import { formatCount } from "@/lib/format";
import { sizeOf, useSizeMode } from "@/lib/size-mode";

interface BarRow {
  key: string;
  label: string;
  color: string;
  value: number;
  files: number;
}

function Bars({ rows, active, onPick }: { rows: BarRow[]; active?: string | null; onPick?: (key: string) => void }) {
  const max = Math.max(1, ...rows.map((r) => r.value));
  return (
    <ul className="m-0 flex list-none flex-col gap-3 p-0">
      {rows.map((r) => {
        const content = (
          <>
            <div className="flex items-baseline gap-2 text-[13px]">
              <span className="truncate">{r.label}</span>
              <span className="num text-xs text-subtle-foreground">{formatCount(r.files)} files</span>
              <SizeDisplay bytes={r.value} className="ml-auto text-foreground-secondary" />
            </div>
            <div className="h-1.5 rounded-full bg-fill-track">
              <div className="h-full rounded-full" style={{ width: `${(r.value / max) * 100}%`, background: r.color }} />
            </div>
          </>
        );
        return (
          <li key={r.key}>
            {onPick ? (
              <button
                type="button"
                aria-pressed={active === r.key}
                onClick={() => onPick(r.key)}
                className={cn(
                  "flex w-full cursor-pointer flex-col gap-1.5 rounded-lg px-2 py-1.5 text-left hover:bg-hover",
                  active === r.key && "bg-muted",
                )}
              >
                {content}
              </button>
            ) : (
              <div className="flex flex-col gap-1.5 px-2 py-1.5">{content}</div>
            )}
          </li>
        );
      })}
    </ul>
  );
}

interface BreakdownProps {
  snapshotId: number;
  node: number;
  category: string | null;
  onCategory: (c: string | null) => void;
}

/** Space by file type (click to filter the files table) or by extension. */
export function Breakdown({ snapshotId, node, category, onCategory }: BreakdownProps) {
  const { measure } = useSizeMode();
  const [tab, setTab] = useState<"types" | "extensions">("types");
  const cats = useCategoriesAt(snapshotId, node);
  const exts = useExtensionsAt(snapshotId, node);
  const q = tab === "types" ? cats : exts;

  const rows: BarRow[] =
    tab === "types"
      ? (cats.data?.data ?? [])
          .map((c) => ({
            key: c.category,
            label: CATEGORY_LABELS[c.category as CategoryKey] ?? c.category,
            color: categoryVar(c.category),
            value: sizeOf(c, measure),
            files: c.file_count,
          }))
          .sort((a, b) => b.value - a.value)
      : [
          ...(exts.data?.data.items ?? []).map((e) => ({
            key: e.ext || "(none)",
            label: e.ext ? `.${e.ext}` : "No extension",
            color: categoryVar(e.category),
            value: sizeOf(e, measure),
            files: e.file_count,
          })),
          ...(exts.data?.data.other
            ? [
                {
                  key: "__other",
                  label: "Everything else",
                  color: categoryVar("other"),
                  value: sizeOf(exts.data.data.other, measure),
                  files: exts.data.data.other.file_count,
                },
              ]
            : []),
        ].sort((a, b) => b.value - a.value);

  return (
    <Card className="flex flex-col">
      <CardHeader>
        <PieChart className="size-4 text-muted-foreground" />
        <CardTitle>By file type</CardTitle>
        <div role="tablist" aria-label="Breakdown" className="ml-auto flex gap-1 text-xs">
          {(["types", "extensions"] as const).map((t) => (
            <button
              key={t}
              type="button"
              role="tab"
              aria-selected={tab === t}
              onClick={() => setTab(t)}
              className={cn("rounded-md px-2.5 py-1", tab === t ? "bg-muted font-medium text-foreground" : "text-muted-foreground hover:bg-hover")}
            >
              {t === "types" ? "Types" : "Extensions"}
            </button>
          ))}
        </div>
      </CardHeader>
      <div className="max-h-[420px] overflow-auto p-3">
        {q.isError ? (
          <ErrorState error={q.error} onRetry={() => q.refetch()} />
        ) : q.isPending ? (
          <div className="flex flex-col gap-4 p-2">
            {Array.from({ length: 7 }, (_, i) => (
              <Skeleton key={i} className="h-7 w-full" />
            ))}
          </div>
        ) : rows.length === 0 ? (
          <EmptyState icon={<PieChart />} title="No files here" className="py-8" />
        ) : (
          <Bars
            rows={rows}
            active={tab === "types" ? category : undefined}
            onPick={tab === "types" ? (k) => onCategory(category === k ? null : k) : undefined}
          />
        )}
      </div>
      {tab === "types" && <p className="m-0 border-t border-border px-5 py-2.5 text-xs text-subtle-foreground">Click a type to filter the files list.</p>}
    </Card>
  );
}
