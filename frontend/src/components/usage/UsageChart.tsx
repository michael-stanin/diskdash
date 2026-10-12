import { useCallback, useMemo } from "react";
import { ChevronRight, FolderOpen, LayoutGrid, Sun } from "lucide-react";
import { useTree } from "@/api/queries";
import type { Crumb } from "@/api/types/Crumb";
import { prefersReducedMotion, useEChart, type ClickParams } from "@/components/charts/useEChart";
import { SizeDisplay } from "@/components/SizeDisplay";
import { EmptyState, ErrorState } from "@/components/States";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { CATEGORY_KEYS, CATEGORY_LABELS, categoryColor, categoryVar, readToken } from "@/lib/chart-theme";
import { cn } from "@/lib/cn";
import { readableOn } from "@/lib/colors";
import { sizeOf, SizeModeToggle, useSizeMode } from "@/lib/size-mode";
import { useTheme } from "@/lib/theme";
import { buildOption, type ChartView, type NodeData } from "./chartOption";

interface UsageChartProps {
  snapshotId: number;
  node: number;
  view: ChartView;
  onView: (v: ChartView) => void;
  onNode: (id: number) => void;
  onOther: (parentId: number, shown: number, count: number) => void;
}

function Breadcrumb({ crumbs, onNode }: { crumbs: Crumb[]; onNode: (id: number) => void }) {
  // Collapse the middle of deep paths; the full path is in each title.
  const shown = crumbs.length > 5 ? [crumbs[0]!, null, ...crumbs.slice(-3)] : crumbs;
  return (
    <nav aria-label="Folder path" className="flex min-w-0 flex-wrap items-center gap-1 text-[13px]">
      {shown.map((c, i) =>
        c === null ? (
          <span key="gap" className="flex items-center gap-1 text-subtle-foreground">
            <ChevronRight className="size-3.5" />…
          </span>
        ) : (
          <span key={c.id} className="flex min-w-0 items-center gap-1">
            {i > 0 && <ChevronRight className="size-3.5 shrink-0 text-subtle-foreground" />}
            {i === shown.length - 1 ? (
              <span aria-current="location" className="truncate font-semibold" title={c.path}>
                {c.name}
              </span>
            ) : (
              <button
                type="button"
                onClick={() => onNode(c.id)}
                title={c.path}
                className="max-w-48 cursor-pointer truncate rounded text-muted-foreground hover:text-foreground"
              >
                {c.name}
              </button>
            )}
          </span>
        ),
      )}
    </nav>
  );
}

/** Treemap / sunburst of one folder level (plus one level of preview). */
export function UsageChart({ snapshotId, node, view, onView, onNode, onOther }: UsageChartProps) {
  const { measure } = useSizeMode();
  const { resolved } = useTheme();
  const tree = useTree(snapshotId, node, measure);
  const data = tree.data?.data;

  const option = useMemo(() => {
    if (!data) return null;
    const palette = {
      color: categoryColor,
      labelOn: (fill: string) => readableOn(fill, readToken("on-color-dark"), readToken("on-color-light")),
    };
    return buildOption(data.node, measure, view, palette, {
      reducedMotion: prefersReducedMotion(),
      gapColor: readToken("card"),
    });
    // `resolved` re-reads the token colours after a theme switch.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data, measure, view, resolved]);

  const onClick = useCallback(
    (p: ClickParams) => {
      const d = p.data as NodeData | undefined;
      if (!d) return;
      if (d.kind === "other" && d.parentId !== undefined) onOther(d.parentId, d.shown ?? 0, d.otherCount ?? 0);
      else if (d.kind === "dir" && d.id !== undefined && !d.incomplete) onNode(d.id);
    },
    [onNode, onOther],
  );
  const chartRef = useEChart(option, onClick);
  const children = data?.node.children ?? [];
  const empty = data && children.length === 0 && !data.node.other;
  const present = new Set(children.map((c) => (c.entry.kind === "dir" ? c.entry.top_category : c.entry.category) ?? "other"));

  return (
    <Card className="flex flex-col gap-3 p-4">
      <div className="flex flex-wrap items-center gap-3">
        {data ? <Breadcrumb crumbs={data.ancestors} onNode={onNode} /> : <Skeleton className="h-5 w-48" />}
        {data && (
          <span className="text-xs text-muted-foreground">
            <SizeDisplay bytes={sizeOf(data.node.entry, measure)} />
          </span>
        )}
        <div className="ml-auto flex items-center gap-2">
          <div role="group" aria-label="Chart type" className="flex overflow-hidden rounded-lg border border-border-strong">
            {(
              [
                ["treemap", LayoutGrid, "Treemap"],
                ["sunburst", Sun, "Sunburst"],
              ] as const
            ).map(([v, Icon, label]) => (
              <button
                key={v}
                type="button"
                aria-pressed={view === v}
                aria-label={label}
                title={label}
                onClick={() => onView(v)}
                className={cn("flex h-8 w-9 items-center justify-center", view === v ? "bg-muted text-foreground" : "text-muted-foreground hover:bg-hover")}
              >
                <Icon className="size-4" />
              </button>
            ))}
          </div>
          <SizeModeToggle />
        </div>
      </div>

      <div className="relative h-[440px]">
        {tree.isError ? (
          <ErrorState title="Cannot load this folder" error={tree.error} onRetry={() => tree.refetch()} />
        ) : empty ? (
          <EmptyState icon={<FolderOpen />} title="This folder is empty" description="Nothing here takes up space." />
        ) : null}
        <div
          ref={chartRef}
          role="img"
          aria-label={data ? `Space used inside ${data.node.entry.path}, largest first` : "Loading chart"}
          className={cn("absolute inset-0", (tree.isError || empty) && "invisible")}
        />
        {!data && !tree.isError && <Skeleton className="absolute inset-0 rounded-xl" />}
        {tree.isFetching && data && <div className="pointer-events-none absolute inset-0 animate-shimmer rounded-xl bg-background/20" />}
      </div>

      {data && (
        <>
          <ul className="sr-only" aria-label="Largest items in this folder">
            {children.map((c) => (
              <li key={c.entry.id}>
                {c.entry.name}: {sizeOf(c.entry, measure)} bytes
              </li>
            ))}
          </ul>
          <div className="flex flex-wrap gap-x-4 gap-y-1.5 text-xs text-muted-foreground" aria-label="Legend">
            {CATEGORY_KEYS.filter((k) => present.has(k)).map((k) => (
              <span key={k} className="flex items-center gap-1.5">
                <span className="size-2.5 rounded-[3px]" style={{ background: categoryVar(k) }} />
                {CATEGORY_LABELS[k]}
              </span>
            ))}
            <span className="ml-auto text-subtle-foreground">
              Up to {data.limit} items per level · folders coloured by their main file type
            </span>
          </div>
        </>
      )}
    </Card>
  );
}
