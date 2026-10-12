import { Copy, FolderOpen, LayoutGrid } from "lucide-react";
import { ApiError } from "@/api/client";
import { useReveal } from "@/api/queries";
import type { EntryView } from "@/api/types/EntryView";
import { useToast } from "@/components/Toast";
import { Button } from "@/components/ui/button";

interface EntryActionsProps {
  snapshotId: number;
  entry: EntryView;
  /** Re-root the chart at a folder. */
  onShow?: (id: number) => void;
}

/** Copy path, Open in Explorer and (folders) show in chart. Read-only actions. */
export function EntryActions({ snapshotId, entry, onShow }: EntryActionsProps) {
  const reveal = useReveal();
  const { toast } = useToast();
  const copy = () => {
    void navigator.clipboard?.writeText(entry.path).then(
      () => toast("Path copied"),
      () => toast("Could not copy to the clipboard", "error"),
    );
  };
  const open = () =>
    reveal.mutate(
      { snapshotId, entryId: entry.id },
      {
        onError: (e) =>
          toast(e instanceof ApiError && e.status === 410 ? "That item no longer exists — rescan to refresh" : e.message, "error"),
      },
    );
  return (
    <>
      {onShow && entry.kind === "dir" && !entry.access_denied && (
        <Button size="icon-sm" variant="outline" aria-label={`Show ${entry.name} in the chart`} title="Show in chart" onClick={() => onShow(entry.id)}>
          <LayoutGrid />
        </Button>
      )}
      <Button size="icon-sm" variant="outline" aria-label={`Open ${entry.name} in Explorer`} title="Open in Explorer" onClick={open}>
        <FolderOpen />
      </Button>
      <Button size="icon-sm" variant="outline" aria-label={`Copy path of ${entry.name}`} title="Copy path" onClick={copy}>
        <Copy />
      </Button>
    </>
  );
}
