import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import { FolderSearch, ScanSearch } from "lucide-react";
import { ApiError } from "@/api/client";
import { useDrives, useStartScan } from "@/api/queries";
import { MediaBadge } from "@/components/Badges";
import { driveName, driveTitle } from "@/components/drives/DriveCard";
import { SizeDisplay } from "@/components/SizeDisplay";
import { EmptyState } from "@/components/States";
import { useToast } from "@/components/Toast";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";

interface ScanDialogContextValue {
  open: (preselect?: string[]) => void;
}

const ScanDialogContext = createContext<ScanDialogContextValue | null>(null);

export function useScanDialog(): ScanDialogContextValue {
  const ctx = useContext(ScanDialogContext);
  if (!ctx) throw new Error("useScanDialog must be used inside ScanDialogProvider");
  return ctx;
}

/** Hosts the single scan dialog; any page can open it (optionally preselecting roots). */
export function ScanDialogProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<{ open: boolean; preselect: string[]; key: number }>({ open: false, preselect: [], key: 0 });
  const open = useCallback(
    (preselect: string[] = []) => setState((s) => ({ open: true, preselect, key: s.key + 1 })),
    [],
  );
  const value = useMemo(() => ({ open }), [open]);
  return (
    <ScanDialogContext.Provider value={value}>
      {children}
      <Dialog open={state.open} onOpenChange={(o) => setState((s) => ({ ...s, open: o }))}>
        {state.open && (
          <ScanForm key={state.key} preselect={state.preselect} onDone={() => setState((s) => ({ ...s, open: false }))} />
        )}
      </Dialog>
    </ScanDialogContext.Provider>
  );
}

function ScanForm({ preselect, onDone }: { preselect: string[]; onDone: () => void }) {
  const drives = useDrives();
  const start = useStartScan();
  const { toast } = useToast();
  const [selected, setSelected] = useState<Set<string>>(new Set(preselect));
  const [folder, setFolder] = useState("");

  const roots = [...selected, ...(folder.trim() ? [folder.trim()] : [])];
  const fields = start.error instanceof ApiError ? start.error.fields : {};
  const folderError = folder.trim() ? fields[`roots[${selected.size}]`] : undefined;
  const otherError = start.error && !(start.error instanceof ApiError && start.error.status === 422) ? start.error.message : null;

  const toggle = (root: string) =>
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(root)) next.delete(root);
      else next.add(root);
      return next;
    });

  const submit = () =>
    start.mutate(roots, {
      onSuccess: (r) => {
        toast(r.snapshots.length === 1 ? `Scanning ${r.snapshots[0]!.root}` : `Scanning ${r.snapshots.length} locations`, "info");
        onDone();
      },
    });

  return (
    <DialogContent
      title="Scan drives"
      description="Read-only: diskdash lists files and sizes; it never opens or changes them."
      footer={
        <>
          <Button variant="ghost" onClick={onDone}>
            Cancel
          </Button>
          <Button variant="primary" disabled={roots.length === 0 || start.isPending} onClick={submit}>
            <ScanSearch /> {start.isPending ? "Starting…" : roots.length > 1 ? `Scan ${roots.length}` : "Start scan"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-5">
        <fieldset className="m-0 flex flex-col gap-2 border-0 p-0">
          <legend className="mb-2 text-[13px] font-medium">Drives</legend>
          {drives.isPending ? (
            Array.from({ length: 3 }, (_, i) => <Skeleton key={i} className="h-14 w-full rounded-xl" />)
          ) : !drives.data || drives.data.drives.length === 0 ? (
            <EmptyState icon={<FolderSearch />} title="No internal drives found" className="py-6" />
          ) : (
            drives.data.drives.map((d) => {
              const root = d.letters[0] ? `${d.letters[0]}:\\` : (d.mount_paths[0] ?? "");
              const checked = selected.has(root);
              return (
                <label
                  key={d.id}
                  className={cn(
                    "flex cursor-pointer items-center gap-3 rounded-xl border px-3.5 py-3 transition-colors",
                    checked ? "border-primary bg-primary-soft/50" : "border-border hover:bg-hover",
                  )}
                >
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={() => toggle(root)}
                    aria-label={`Scan ${driveName(d)} ${driveTitle(d)}`}
                    className="size-4 accent-primary"
                  />
                  <span className="w-7 font-semibold">{driveName(d)}</span>
                  <span className="min-w-0 flex-1 truncate text-[13px]">{driveTitle(d)}</span>
                  <span className="text-xs text-muted-foreground">
                    <SizeDisplay bytes={d.used} /> used
                  </span>
                  <MediaBadge media={d.media} />
                </label>
              );
            })
          )}
          {fields.roots && <span className="text-xs text-danger">{fields.roots}</span>}
        </fieldset>

        <div className="flex flex-col gap-1.5">
          <label htmlFor="scan-folder" className="text-[13px] font-medium">
            Or a single folder
          </label>
          <Input
            id="scan-folder"
            placeholder="D:\Media"
            value={folder}
            onChange={(e) => setFolder(e.target.value)}
            aria-invalid={Boolean(folderError)}
            className="font-mono"
          />
          {folderError ? (
            <span role="alert" className="text-xs text-danger">
              {folderError}
            </span>
          ) : (
            <span className="text-xs text-muted-foreground">Hard disks are scanned one folder at a time to avoid slowing them down.</span>
          )}
          {otherError && (
            <span role="alert" className="text-xs text-danger">
              {otherError}
            </span>
          )}
        </div>
      </div>
    </DialogContent>
  );
}
