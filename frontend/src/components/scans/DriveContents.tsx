import { useMemo, useState } from "react";
import type { ColumnDef } from "@tanstack/react-table";
import { Copy, FileText, Folder, ScanSearch } from "lucide-react";
import { useTopDirs, useTopFiles } from "@/api/queries";
import type { EntryView } from "@/api/types/EntryView";
import type { Measure } from "@/api/types/Measure";
import type { Snapshot } from "@/api/types/Snapshot";
import { DataTable } from "@/components/DataTable";
import { RelativeTime, SizeDisplay } from "@/components/SizeDisplay";
import { EmptyState, ErrorState } from "@/components/States";
import { useToast } from "@/components/Toast";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { CATEGORY_LABELS, type CategoryKey } from "@/lib/chart-theme";
import { cn } from "@/lib/cn";
import { formatCount, formatDuration } from "@/lib/format";

function useColumns(measure: Measure, kind: "dirs" | "files"): ColumnDef<EntryView, unknown>[] {
  return useMemo(() => {
    const cols: ColumnDef<EntryView, unknown>[] = [
      { accessorKey: "path", header: "Path", meta: { mono: true } },
      {
        id: "size",
        accessorFn: (e) => (measure === "alloc" ? e.alloc : e.size),
        header: measure === "alloc" ? "Size on disk" : "Size",
        meta: { align: "right", width: "110px" },
        cell: (c) => <SizeDisplay bytes={c.getValue<number>()} className="font-semibold" />,
      },
    ];
    if (kind === "dirs") {
      cols.push({
        id: "files",
        accessorFn: (e) => e.file_count ?? 0,
        header: "Files",
        meta: { align: "right", width: "92px" },
        cell: (c) => formatCount(c.getValue<number>()),
      });
    } else {
      cols.push({
        id: "type",
        accessorFn: (e) => (e.category ? CATEGORY_LABELS[e.category as CategoryKey] : ""),
        header: "Type",
        meta: { width: "120px" },
      });
    }
    return cols;
  }, [measure, kind]);
}

function TopTable({ title, icon, rows, loading, error, retry, kind, measure }: {
  title: string;
  icon: React.ReactNode;
  rows?: EntryView[];
  loading: boolean;
  error: unknown;
  retry: () => void;
  kind: "dirs" | "files";
  measure: Measure;
}) {
  const columns = useColumns(measure, kind);
  const { toast } = useToast();
  const copy = (path: string) => {
    void navigator.clipboard?.writeText(path).then(() => toast("Path copied"));
  };
  return (
    <Card className="overflow-hidden">
      <CardHeader>
        <span className="text-muted-foreground [&_svg]:size-4">{icon}</span>
        <CardTitle>{title}</CardTitle>
      </CardHeader>
      {error ? (
        <ErrorState title={`Cannot load ${title.toLowerCase()}`} error={error} onRetry={retry} />
      ) : loading || !rows ? (
        <div className="flex flex-col gap-3 p-5">
          {Array.from({ length: 6 }, (_, i) => (
            <Skeleton key={i} className="h-5 w-full" />
          ))}
        </div>
      ) : (
        <DataTable
          ariaLabel={title}
          columns={columns}
          data={rows}
          getRowId={(r) => String(r.id)}
          height={360}
          rowActions={(r) => (
            <Button size="icon-sm" variant="outline" aria-label={`Copy path ${r.path}`} onClick={() => copy(r.path)}>
              <Copy />
            </Button>
          )}
        />
      )}
    </Card>
  );
}

interface DriveContentsProps {
  snapshot?: Snapshot;
  onScan: () => void;
  scanning: boolean;
}

/** Largest folders and files from the drive's latest complete scan. */
export function DriveContents({ snapshot, onScan, scanning }: DriveContentsProps) {
  const [measure, setMeasure] = useState<Measure>("alloc");
  const dirs = useTopDirs(snapshot?.id, measure, 20, undefined, 3);
  const files = useTopFiles(snapshot?.id, measure, 20);

  if (!snapshot) {
    return (
      <Card>
        <EmptyState
          icon={<ScanSearch />}
          title={scanning ? "Scanning…" : "Not scanned yet"}
          description="A scan lists every folder and file on this drive (read-only) so diskdash can show where the space goes."
          action={
            !scanning && (
              <Button variant="primary" onClick={onScan}>
                <ScanSearch /> Scan this drive
              </Button>
            )
          }
        />
      </Card>
    );
  }

  const duration =
    snapshot.finished_at && (Date.parse(snapshot.finished_at) - Date.parse(snapshot.started_at)) / 1000;

  return (
    <section aria-label="Contents" className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <h2 className="m-0 text-[15px] font-semibold text-foreground-secondary">Contents</h2>
        <span className="text-xs text-muted-foreground">
          {formatCount(snapshot.file_count)} files · scanned <RelativeTime date={snapshot.finished_at ?? snapshot.started_at} />
          {duration ? ` in ${formatDuration(duration)}` : ""}
          {snapshot.error_count > 0 ? ` · ${formatCount(snapshot.error_count)} unreadable` : ""}
        </span>
        {!snapshot.hardlinks_deduplicated && (
          <span className="text-xs text-subtle-foreground" title="Hard-linked files may be counted more than once on this drive. Turn on detection per drive in Settings.">
            · hard links not de-duplicated
          </span>
        )}
        <div role="group" aria-label="Size measure" className="ml-auto flex overflow-hidden rounded-lg border border-border-strong text-xs">
          {(["alloc", "size"] as const).map((m) => (
            <button
              key={m}
              type="button"
              aria-pressed={measure === m}
              onClick={() => setMeasure(m)}
              className={cn(
                "h-8 px-3",
                measure === m ? "bg-muted font-medium text-foreground" : "text-muted-foreground hover:bg-hover",
              )}
            >
              {m === "alloc" ? "Size on disk" : "Logical size"}
            </button>
          ))}
        </div>
        <Button variant="secondary" size="sm" onClick={onScan} disabled={scanning}>
          <ScanSearch /> Rescan
        </Button>
      </div>
      <div className="grid grid-cols-1 gap-4 2xl:grid-cols-2">
        <TopTable
          title="Largest folders"
          icon={<Folder />}
          rows={dirs.data?.data}
          loading={dirs.isPending}
          error={dirs.error}
          retry={() => dirs.refetch()}
          kind="dirs"
          measure={measure}
        />
        <TopTable
          title="Largest files"
          icon={<FileText />}
          rows={files.data?.data}
          loading={files.isPending}
          error={files.error}
          retry={() => files.refetch()}
          kind="files"
          measure={measure}
        />
      </div>
    </section>
  );
}
