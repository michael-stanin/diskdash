import { Loader2, Square } from "lucide-react";
import { useEvents } from "@/api/events";
import { useCancelJob } from "@/api/queries";
import type { JobStatus } from "@/api/types/JobStatus";
import { FillBar } from "@/components/FillBar";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { formatBytes, formatDuration } from "@/lib/format";

function ScanRow({ job }: { job: JobStatus }) {
  const cancel = useCancelJob();
  const fraction = job.total ? Math.min(1, job.done / job.total) : null;
  const waiting = job.status === "queued";
  return (
    <div className="flex flex-col gap-2.5 px-5 py-4">
      <div className="flex items-center gap-3">
        <Loader2 className={waiting ? "size-4 text-muted-foreground" : "size-4 animate-spin text-primary"} />
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium" title={job.phase ?? undefined}>
          {waiting ? (job.phase ?? "Waiting to start") : (job.phase ?? "Scanning")}
        </span>
        <span className="num text-xs text-muted-foreground">
          {formatBytes(job.done)}
          {job.total ? ` of ~${formatBytes(job.total)}` : ""}
          {job.eta_seconds !== null && !waiting ? ` · about ${formatDuration(job.eta_seconds)} left` : ""}
        </span>
        <Button
          size="sm"
          variant="secondary"
          disabled={cancel.isPending}
          onClick={() => cancel.mutate(job.id)}
          aria-label={`Cancel scan ${job.id}`}
        >
          <Square /> Cancel
        </Button>
      </div>
      {fraction !== null ? (
        <FillBar usedFraction={fraction} state="normal" label="Scan progress" className="h-1.5" />
      ) : (
        <div className="h-1.5 overflow-hidden rounded-full bg-fill-track" aria-label="Scan in progress">
          <div className="h-full w-1/3 animate-shimmer rounded-full bg-fill-normal" />
        </div>
      )}
    </div>
  );
}

/** Live scans (from the event stream). Renders nothing when idle. */
export function ScanActivity() {
  const { activeJobs } = useEvents();
  const scans = activeJobs.filter((j) => j.kind === "scan");
  if (scans.length === 0) return null;
  return (
    <Card className="divide-y divide-border" aria-label="Scans in progress">
      {scans.map((j) => (
        <ScanRow key={j.id} job={j} />
      ))}
    </Card>
  );
}
