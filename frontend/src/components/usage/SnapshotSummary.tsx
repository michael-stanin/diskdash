import { ScanSearch } from "lucide-react";
import type { Snapshot } from "@/api/types/Snapshot";
import { RelativeTime } from "@/components/SizeDisplay";
import { Button } from "@/components/ui/button";
import { formatCount, formatDuration } from "@/lib/format";

/** "1,165,878 files · scanned 2 days ago in 2 min 39 s · 478 unreadable" + Rescan. */
export function SnapshotSummary({ snapshot: s, onRescan, scanning }: { snapshot: Snapshot; onRescan?: () => void; scanning?: boolean }) {
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
      {!s.hardlinks_deduplicated && (
        <span title="Hard-linked files may be counted more than once on this drive. Turn on detection per drive in Settings.">
          · hard links not de-duplicated
        </span>
      )}
      {onRescan && (
        <Button variant="secondary" size="sm" className="ml-auto" onClick={onRescan} disabled={scanning}>
          <ScanSearch /> {scanning ? "Scanning…" : "Rescan"}
        </Button>
      )}
    </div>
  );
}
