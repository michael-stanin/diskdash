import { ScanSearch } from "lucide-react";
import type { Snapshot } from "@/api/types/Snapshot";
import { ModeBadge, QuickRescanButton } from "@/components/scans/ScanMode";
import { RelativeTime } from "@/components/SizeDisplay";
import { Button } from "@/components/ui/button";
import { formatCount, formatDuration } from "@/lib/format";

/** "1,165,878 files · scanned 2 days ago in 2 min 39 s · 478 unreadable" + Rescan. */
export function SnapshotSummary({
  snapshot: s,
  onRescan,
  scanning,
  quickRescanRoot,
}: {
  snapshot: Snapshot;
  onRescan?: () => void;
  scanning?: boolean;
  /** Set when the elevated helper is running and this snapshot came from an MFT scan. */
  quickRescanRoot?: string;
}) {
  const duration = s.finished_at ? (Date.parse(s.finished_at) - Date.parse(s.started_at)) / 1000 : null;
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
      <span className="num">
        {formatCount(s.file_count)} files · {formatCount(s.dir_count)} folders · scanned{" "}
        <RelativeTime date={s.finished_at ?? s.started_at} />
        {duration !== null && duration > 0 ? ` in ${formatDuration(duration)}` : ""}
      </span>
      {s.error_count > 0 && (
        <span title="Folders Windows did not let diskdash read (mostly system folders without admin rights). Their sizes are not included.">
          · {formatCount(s.error_count)} unreadable
        </span>
      )}
      <span title={s.scan_note ?? undefined}>
        <ModeBadge method={s.method} />
      </span>
      {s.scan_note && s.method === "walk" && <span className="text-subtle-foreground">· {s.scan_note}</span>}
      {!s.hardlinks_deduplicated && (
        <span title="Hard-linked files may be counted more than once on this drive. Turn on detection per drive in Settings.">
          · hard links not de-duplicated
        </span>
      )}
      {(onRescan || quickRescanRoot) && (
        <span className="ml-auto flex items-start gap-2">
          {quickRescanRoot && !scanning && <QuickRescanButton root={quickRescanRoot} />}
          {onRescan && (
            <Button variant="secondary" size="sm" onClick={onRescan} disabled={scanning}>
              <ScanSearch /> {scanning ? "Scanning…" : "Rescan"}
            </Button>
          )}
        </span>
      )}
    </div>
  );
}
