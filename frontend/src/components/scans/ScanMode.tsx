import { useState } from "react";
import { Gauge, ShieldCheck, ShieldAlert, Zap } from "lucide-react";
import { ApiError } from "@/api/client";
import { useEnableHelper, useQuickRescan, useScanModes } from "@/api/queries";
import type { HelperStatus } from "@/api/types/HelperStatus";
import { useToast } from "@/components/Toast";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";

const LABEL: Record<string, string> = {
  mft: "fast scan (MFT)",
  "mft-quick": "quick rescan (MFT + change journal)",
  walk: "directory walk",
};

/** Which way a drive is (or was) scanned. */
export function ModeBadge({ method, className }: { method: string; className?: string }) {
  const fast = method !== "walk";
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-md px-2 py-0.5 text-[11px] font-semibold whitespace-nowrap",
        fast ? "bg-primary-soft text-primary-soft-foreground" : "bg-muted text-foreground-secondary",
        className,
      )}
    >
      {fast ? <Zap className="size-3" aria-hidden /> : <Gauge className="size-3" aria-hidden />}
      {LABEL[method] ?? method}
    </span>
  );
}

/** Explains the fast scan and starts the elevated helper on request (shows the Windows prompt). */
export function ElevatedFeatures() {
  const modes = useScanModes();
  const enable = useEnableHelper();
  const [shown, setShown] = useState(false);
  const helper: HelperStatus | undefined = modes.data?.helper;

  if (modes.isPending) return <Skeleton className="h-16 w-full rounded-xl" />;
  if (!helper) return null;

  const failure = enable.error instanceof Error ? enable.error.message : helper.state === "failed" ? helper.message : null;
  const running = helper.state === "running";
  return (
    <section aria-label="Elevated features" className="flex flex-col gap-2 rounded-xl border border-border p-3.5">
      <div className="flex items-center gap-2.5">
        {running ? <ShieldCheck className="size-4 text-success" aria-hidden /> : <ShieldAlert className="size-4 text-muted-foreground" aria-hidden />}
        <span className="text-[13px] font-medium">
          {running ? "Elevated helper running" : helper.state === "unavailable" ? "Fast scan is not installed" : "Fast scan needs administrator rights"}
        </span>
        {!running && helper.state !== "unavailable" && (
          <Button
            size="sm"
            variant="secondary"
            className="ml-auto"
            disabled={enable.isPending}
            onClick={() => {
              setShown(true);
              enable.mutate();
            }}
          >
            {enable.isPending ? "Waiting for the Windows prompt…" : "Enable elevated features"}
          </Button>
        )}
      </div>
      <p className="m-0 text-xs leading-relaxed text-muted-foreground">
        {running
          ? "Whole NTFS drives are read from the Master File Table in seconds. The helper only reads; it never changes anything."
          : helper.state === "unavailable"
            ? helper.message ?? "diskdash-helper.exe was not found next to diskdash.exe."
            : "Windows asks once. The helper only reads the drive's file table and change journal; it cannot write, delete or run anything. Without it, scans walk the folders, which is slower but needs no rights."}
      </p>
      {enable.isPending && (
        <p role="status" className="m-0 text-xs text-muted-foreground">
          Approve the Windows administrator prompt to continue.
        </p>
      )}
      {failure && (shown || helper.state === "failed") && (
        <p role="alert" className="m-0 rounded-lg bg-danger-soft px-3 py-2 text-xs text-danger-soft-foreground">
          {failure} Scans keep using the directory walk.
        </p>
      )}
    </section>
  );
}

/** Re-reads only what changed since the last fast scan. */
export function QuickRescanButton({ root }: { root: string }) {
  const quick = useQuickRescan();
  const { toast } = useToast();
  const error = quick.error instanceof ApiError ? quick.error.message : quick.error?.message;
  return (
    <span className="inline-flex flex-col items-end gap-1">
      <Button
        variant="secondary"
        size="sm"
        disabled={quick.isPending}
        onClick={() =>
          quick.mutate(root, {
            onSuccess: (r) => {
              const s = r.snapshots[0];
              toast(s?.method === "mft-quick" ? `Quick rescan of ${root}` : (s?.note ?? `Scanning ${root}`), "info");
            },
          })
        }
      >
        <Zap /> {quick.isPending ? "Checking changes…" : "Quick rescan"}
      </Button>
      {error && (
        <span role="alert" className="max-w-xs text-right text-xs text-danger">
          {error}
        </span>
      )}
    </span>
  );
}
