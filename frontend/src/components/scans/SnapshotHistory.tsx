import { useState } from "react";
import { History, Play, Trash2 } from "lucide-react";
import { useDeleteSnapshot, useResumeScan } from "@/api/queries";
import type { Snapshot } from "@/api/types/Snapshot";
import type { SnapshotStatus } from "@/api/types/SnapshotStatus";
import { RelativeTime, SizeDisplay } from "@/components/SizeDisplay";
import { EmptyState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";
import { formatCount } from "@/lib/format";

const STATUS: Record<SnapshotStatus, { label: string; className: string }> = {
  complete: { label: "Complete", className: "bg-risk-safe-bg text-risk-safe-fg" },
  running: { label: "Running", className: "bg-primary-soft text-primary-soft-foreground" },
  partial: { label: "Stopped", className: "bg-risk-review-bg text-risk-review-fg" },
  failed: { label: "Failed", className: "bg-risk-expert-bg text-risk-expert-fg" },
};

export function SnapshotHistory({ snapshots, loading }: { snapshots?: Snapshot[]; loading: boolean }) {
  const resume = useResumeScan();
  const remove = useDeleteSnapshot();
  const [confirm, setConfirm] = useState<Snapshot | null>(null);

  return (
    <Card className="overflow-hidden">
      <CardHeader>
        <History className="size-4 text-muted-foreground" />
        <CardTitle>Scan history</CardTitle>
      </CardHeader>
      {loading ? (
        <div className="flex flex-col gap-3 p-5">
          {Array.from({ length: 3 }, (_, i) => (
            <Skeleton key={i} className="h-6 w-full" />
          ))}
        </div>
      ) : !snapshots || snapshots.length === 0 ? (
        <EmptyState icon={<History />} title="No scans yet" className="py-8" />
      ) : (
        <ul className="m-0 list-none divide-y divide-border p-0" aria-label="Scans">
          {snapshots.map((s) => (
            <li key={s.id} className="flex flex-wrap items-center gap-x-4 gap-y-1 px-5 py-3 text-[13px]">
              <span className={cn("rounded-md px-2 py-0.5 text-[11px] font-semibold", STATUS[s.status].className)}>
                {STATUS[s.status].label}
              </span>
              <span className="font-mono text-xs text-foreground-secondary">{s.root}</span>
              <span className="text-muted-foreground">
                <RelativeTime date={s.started_at} />
              </span>
              {s.status === "complete" && (
                <span className="num text-muted-foreground">
                  {formatCount(s.file_count)} files · <SizeDisplay bytes={s.total_alloc} />
                </span>
              )}
              {s.error && s.status !== "complete" && <span className="text-xs text-muted-foreground">{s.error}</span>}
              <span className="ml-auto flex gap-1.5">
                {s.status === "partial" && (
                  <Button size="sm" variant="secondary" disabled={resume.isPending} onClick={() => resume.mutate(s.id)}>
                    <Play /> Resume
                  </Button>
                )}
                {s.status !== "running" && (
                  <Button size="icon-sm" variant="ghost" aria-label={`Remove scan ${s.id}`} onClick={() => setConfirm(s)}>
                    <Trash2 />
                  </Button>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
      <Dialog open={confirm !== null} onOpenChange={(o) => !o && setConfirm(null)}>
        {confirm && (
          <DialogContent
            title="Remove this scan?"
            description="This removes diskdash's saved record of the scan. Your files are not touched."
            footer={
              <>
                <Button variant="ghost" onClick={() => setConfirm(null)}>
                  Keep
                </Button>
                <Button
                  variant="primary"
                  disabled={remove.isPending}
                  onClick={() => remove.mutate(confirm.id, { onSuccess: () => setConfirm(null) })}
                >
                  Remove record
                </Button>
              </>
            }
          >
            <p className="m-0 text-[13px] text-muted-foreground">
              Scan of <span className="font-mono">{confirm.root}</span> from <RelativeTime date={confirm.started_at} />.
            </p>
          </DialogContent>
        )}
      </Dialog>
    </Card>
  );
}
