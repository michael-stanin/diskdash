import { useMemo } from "react";
import type { ColumnDef } from "@tanstack/react-table";
import { useChildrenInfinite } from "@/api/queries";
import type { EntryView } from "@/api/types/EntryView";
import { DataTable } from "@/components/DataTable";
import { SizeDisplay } from "@/components/SizeDisplay";
import { ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { formatCount } from "@/lib/format";
import { sizeOf, useSizeMode } from "@/lib/size-mode";
import { EntryActions } from "./EntryActions";

export interface OtherTarget {
  parentId: number;
  shown: number;
  count: number;
}

interface OtherDialogProps {
  snapshotId: number;
  target: OtherTarget | null;
  onClose: () => void;
  onShow: (id: number) => void;
}

/** Everything folded into "Other": paged, and any folder can be opened in the chart. */
export function OtherDialog({ snapshotId, target, onClose, onShow }: OtherDialogProps) {
  const { measure } = useSizeMode();
  const q = useChildrenInfinite(snapshotId, target?.parentId ?? null, measure, target?.shown ?? 0);
  const rows = useMemo(() => q.data?.pages.flatMap((p) => p.data.items) ?? [], [q.data]);
  const columns = useMemo<ColumnDef<EntryView, unknown>[]>(
    () => [
      { accessorKey: "name", header: "Name", meta: { mono: true } },
      {
        id: "size",
        accessorFn: (e) => sizeOf(e, measure),
        header: "Size",
        meta: { align: "right", width: "104px" },
        cell: (c) => <SizeDisplay bytes={c.getValue<number>()} className="font-semibold" />,
      },
    ],
    [measure],
  );
  const show = (id: number) => {
    onShow(id);
    onClose();
  };

  return (
    <Dialog open={target !== null} onOpenChange={(o) => !o && onClose()}>
      {target && (
        <DialogContent
          title={`${formatCount(target.count)} smaller items`}
          description="Items beyond the chart's per-level limit, largest first."
          className="w-[min(760px,calc(100vw-32px))]"
        >
          {q.isError ? (
            <ErrorState error={q.error} onRetry={() => q.refetch()} />
          ) : q.isPending ? (
            <div className="flex flex-col gap-3">
              {Array.from({ length: 6 }, (_, i) => (
                <Skeleton key={i} className="h-5 w-full" />
              ))}
            </div>
          ) : (
            <div className="-mx-6 -my-5">
              <DataTable
                ariaLabel="Smaller items"
                columns={columns}
                data={rows}
                getRowId={(r) => String(r.id)}
                height={380}
                rowActions={(r) => <EntryActions snapshotId={snapshotId} entry={r} onShow={show} />}
              />
              {q.hasNextPage && (
                <div className="border-t border-border px-4 py-2.5">
                  <Button variant="ghost" size="sm" disabled={q.isFetchingNextPage} onClick={() => void q.fetchNextPage()}>
                    {q.isFetchingNextPage ? "Loading…" : "Show more"}
                  </Button>
                </div>
              )}
            </div>
          )}
        </DialogContent>
      )}
    </Dialog>
  );
}
