import { useMemo, type ReactNode } from "react";
import type { ColumnDef } from "@tanstack/react-table";
import { FileText, Folder, X } from "lucide-react";
import { useTopInfinite } from "@/api/queries";
import type { EntryView } from "@/api/types/EntryView";
import { DataTable } from "@/components/DataTable";
import { RelativeTime, SizeDisplay } from "@/components/SizeDisplay";
import { EmptyState, ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { CATEGORY_LABELS, categoryVar, type CategoryKey } from "@/lib/chart-theme";
import { formatCount } from "@/lib/format";
import { sizeOf, useSizeMode } from "@/lib/size-mode";
import { EntryActions } from "./EntryActions";

function CategoryChip({ category }: { category: string | null }) {
  if (!category) return null;
  return (
    <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
      <span className="size-2 rounded-full" style={{ background: categoryVar(category) }} />
      {CATEGORY_LABELS[category as CategoryKey] ?? category}
    </span>
  );
}

interface TopTableProps {
  snapshotId: number;
  node: number;
  kind: "dirs" | "files";
  category?: string | null;
  onClearCategory?: () => void;
  onShow?: (id: number) => void;
}

export function TopTable({ snapshotId, node, kind, category, onClearCategory, onShow }: TopTableProps) {
  const { measure } = useSizeMode();
  const q = useTopInfinite(snapshotId, kind, node, measure, category);
  const rows = useMemo(() => q.data?.pages.flatMap((p) => p.data) ?? [], [q.data]);

  const columns = useMemo<ColumnDef<EntryView, unknown>[]>(() => {
    const cols: ColumnDef<EntryView, unknown>[] = [
      {
        accessorKey: "path",
        header: "Path",
        meta: { mono: true },
        cell: (c) => (
          <span title={c.row.original.path}>
            {c.row.original.path}
            {c.row.original.access_denied && <span className="ml-2 font-sans text-[11px] text-warning">incomplete</span>}
          </span>
        ),
      },
      {
        id: "size",
        accessorFn: (e) => sizeOf(e, measure),
        header: measure === "alloc" ? "Size on disk" : "Size",
        meta: { align: "right", width: "104px" },
        cell: (c) => <SizeDisplay bytes={c.getValue<number>()} className="font-semibold" />,
      },
    ];
    if (kind === "dirs") {
      cols.push({
        id: "files",
        accessorFn: (e) => e.file_count ?? 0,
        header: "Files",
        meta: { align: "right", width: "84px" },
        cell: (c) => formatCount(c.getValue<number>()),
      });
    } else {
      cols.push(
        {
          id: "type",
          accessorFn: (e) => e.category ?? "",
          header: "Type",
          meta: { width: "128px" },
          cell: (c) => <CategoryChip category={c.row.original.category} />,
        },
        {
          id: "modified",
          accessorFn: (e) => e.modified ?? "",
          header: "Modified",
          meta: { width: "120px" },
          cell: (c) => (c.row.original.modified ? <RelativeTime date={c.row.original.modified} className="text-xs text-muted-foreground" /> : "—"),
        },
      );
    }
    return cols;
  }, [kind, measure]);

  const title = kind === "dirs" ? "Largest folders" : "Largest files";
  const icon: ReactNode = kind === "dirs" ? <Folder /> : <FileText />;

  return (
    <Card className="flex flex-col overflow-hidden">
      <CardHeader>
        <span className="text-muted-foreground [&_svg]:size-4">{icon}</span>
        <CardTitle>{title}</CardTitle>
        {category && onClearCategory && (
          <button
            type="button"
            onClick={onClearCategory}
            className="ml-1 inline-flex items-center gap-1.5 rounded-full border border-border-strong px-2.5 py-0.5 text-xs hover:bg-hover"
            aria-label={`Clear filter ${CATEGORY_LABELS[category as CategoryKey] ?? category}`}
          >
            <span className="size-2 rounded-full" style={{ background: categoryVar(category) }} />
            {CATEGORY_LABELS[category as CategoryKey] ?? category}
            <X className="size-3" />
          </button>
        )}
      </CardHeader>
      {q.isError ? (
        <ErrorState title={`Cannot load ${title.toLowerCase()}`} error={q.error} onRetry={() => q.refetch()} />
      ) : q.isPending ? (
        <div className="flex flex-col gap-3 p-5" aria-label={`Loading ${title.toLowerCase()}`}>
          {Array.from({ length: 6 }, (_, i) => (
            <Skeleton key={i} className="h-5 w-full" />
          ))}
        </div>
      ) : rows.length === 0 ? (
        <EmptyState
          icon={icon}
          title={category ? "No files of this type here" : kind === "dirs" ? "No subfolders" : "No files here"}
          className="py-10"
        />
      ) : (
        <>
          <DataTable
            ariaLabel={title}
            columns={columns}
            data={rows}
            getRowId={(r) => String(r.id)}
            height={360}
            rowActions={(r) => <EntryActions snapshotId={snapshotId} entry={r} onShow={onShow} />}
          />
          {q.hasNextPage && (
            <div className="border-t border-border px-4 py-2.5">
              <Button variant="ghost" size="sm" disabled={q.isFetchingNextPage} onClick={() => void q.fetchNextPage()}>
                {q.isFetchingNextPage ? "Loading…" : "Show more"}
              </Button>
            </div>
          )}
        </>
      )}
    </Card>
  );
}
