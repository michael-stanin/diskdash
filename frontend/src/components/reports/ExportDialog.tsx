import { useMemo, useState } from "react";
import { Download, FileDown, FolderOutput, ScanSearch } from "lucide-react";
import { ApiError } from "@/api/client";
import { useDownloadReport, useDrives, useReportSections, useSaveReport } from "@/api/queries";
import type { Format } from "@/api/types/Format";
import type { ReportRequest } from "@/api/types/ReportRequest";
import type { SavedReport } from "@/api/types/SavedReport";
import type { SectionId } from "@/api/types/SectionId";
import { useScanDialog } from "@/components/scans/ScanDialog";
import { ErrorState } from "@/components/States";
import { useToast } from "@/components/Toast";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { Input, Select } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";

const FORMATS: { key: Format; label: string; hint: string }[] = [
  { key: "html", label: "HTML", hint: "One page you can open anywhere, offline. Charts included." },
  { key: "csv", label: "CSV (zip)", hint: "One spreadsheet file per section, in a zip." },
  { key: "json", label: "JSON", hint: "For other tools. Has a schema version." },
];

/** "Export report" button with its dialog. `drive` limits the report to one drive letter. */
export function ExportButton({ drive, className }: { drive?: string; className?: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="secondary" className={className} onClick={() => setOpen(true)}>
        <FileDown /> Export report
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        {open && <ExportForm initialDrive={drive} onDone={() => setOpen(false)} />}
      </Dialog>
    </>
  );
}

function message(e: unknown): string {
  if (e instanceof ApiError) return Object.values(e.fields)[0] ?? e.message;
  return e instanceof Error ? e.message : String(e);
}

function ExportForm({ initialDrive, onDone }: { initialDrive?: string; onDone: () => void }) {
  const drives = useDrives();
  const [drive, setDrive] = useState(initialDrive ?? "");
  const sections = useReportSections(drive || undefined);
  const download = useDownloadReport();
  const save = useSaveReport();
  const scanDialog = useScanDialog();
  const { toast } = useToast();
  const [format, setFormat] = useState<Format>("html");
  const [picked, setPicked] = useState<Set<SectionId> | null>(null);
  const [folder, setFolder] = useState("");
  const [saved, setSaved] = useState<SavedReport | null>(null);
  const [last, setLast] = useState<"download" | "save">("download");

  const list = useMemo(() => sections.data ?? [], [sections.data]);
  const availableIds = useMemo(() => new Set(list.filter((s) => s.available).map((s) => s.id)), [list]);
  const chosen = useMemo(() => (picked ? new Set([...picked].filter((id) => availableIds.has(id))) : availableIds), [picked, availableIds]);
  // Without a scan only the drive summary (and the free-space history) can be exported.
  const needsScan = list.length > 0 && !(["top", "stale", "cleanup"] as SectionId[]).some((id) => availableIds.has(id));
  const availableNames = list.filter((s) => s.available).map((s) => `the ${s.title.toLowerCase()}`);
  const busy = download.isPending || save.isPending;
  const error = last === "download" ? download.error : save.error;

  const request = (): Omit<ReportRequest, "folder"> => ({
    format,
    sections: list.filter((s) => chosen.has(s.id)).map((s) => s.id),
    drive: drive || null,
    top_n: null,
  });
  const runDownload = () => {
    setLast("download");
    setSaved(null);
    download.mutate(request(), {
      onSuccess: (name) => {
        toast(`Report downloaded: ${name}`);
        onDone();
      },
    });
  };
  const runSave = () => {
    setLast("save");
    setSaved(null);
    save.mutate({ ...request(), folder }, { onSuccess: (r) => setSaved(r) });
  };
  const toggle = (id: SectionId) =>
    setPicked((p) => {
      const next = new Set(p ?? availableIds);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <DialogContent
      title="Export report"
      description="A read-only snapshot of what diskdash found. Nothing in it runs anything; steps are text for you to follow."
      footer={
        <>
          <Button variant="ghost" onClick={onDone}>
            Close
          </Button>
          <Button variant="primary" disabled={busy || chosen.size === 0} onClick={runDownload}>
            <Download /> {download.isPending ? "Creating report…" : "Download"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-5">
        <label className="flex flex-col gap-1.5 text-[13px] font-medium">
          Drive
          <Select value={drive} onChange={(e) => { setDrive(e.target.value); setPicked(null); }} aria-label="Drive">
            <option value="">All drives</option>
            {(drives.data?.drives ?? []).map((d) => (d.letters[0] ? <option key={d.id} value={d.letters[0]}>{`${d.letters[0]}: ${d.label || "Local disk"}`}</option> : null))}
          </Select>
        </label>

        <fieldset className="m-0 flex flex-col gap-2 border-0 p-0">
          <legend className="mb-1 text-[13px] font-medium">Sections</legend>
          {sections.isPending ? (
            Array.from({ length: 4 }, (_, i) => <Skeleton key={i} className="h-9 w-full rounded-lg" />)
          ) : sections.isError ? (
            <ErrorState title="Could not check what is available" error={sections.error} onRetry={() => void sections.refetch()} className="py-6" />
          ) : (
            <>
              {needsScan && (
                <div className="flex flex-wrap items-center gap-3 rounded-xl bg-muted px-3.5 py-3 text-[13px]">
                  <ScanSearch className="size-4 text-muted-foreground" aria-hidden />
                  <span className="min-w-0 flex-1">
                    Nothing has been scanned yet, so only {availableNames.join(" and ") || "the drive summary"} {availableNames.length > 1 ? "are" : "is"} available.
                  </span>
                  <Button size="sm" variant="secondary" onClick={() => { onDone(); scanDialog.open(); }}>
                    Scan a drive
                  </Button>
                </div>
              )}
              {list.map((s) => (
                <label
                  key={s.id}
                  className={cn(
                    "flex items-start gap-3 rounded-lg border border-border px-3 py-2 text-[13px]",
                    s.available ? "cursor-pointer hover:bg-hover" : "opacity-70",
                  )}
                >
                  <input
                    type="checkbox"
                    className="mt-0.5 size-4 accent-primary"
                    checked={chosen.has(s.id)}
                    disabled={!s.available}
                    onChange={() => toggle(s.id)}
                    aria-label={s.title}
                  />
                  <span className="flex min-w-0 flex-col">
                    <span className="font-medium">{s.title}</span>
                    {!s.available && <span className="text-xs text-muted-foreground">Not available: {s.reason}</span>}
                  </span>
                </label>
              ))}
            </>
          )}
        </fieldset>

        <fieldset className="m-0 flex flex-col gap-2 border-0 p-0">
          <legend className="mb-1 text-[13px] font-medium">Format</legend>
          <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
            {FORMATS.map((f) => (
              <label
                key={f.key}
                className={cn(
                  "flex cursor-pointer flex-col gap-0.5 rounded-lg border px-3 py-2 text-[13px]",
                  format === f.key ? "border-primary bg-primary-soft/50" : "border-border hover:bg-hover",
                )}
              >
                <span className="flex items-center gap-2 font-medium">
                  <input type="radio" name="report-format" className="size-4 accent-primary" checked={format === f.key} onChange={() => setFormat(f.key)} />
                  {f.label}
                </span>
                <span className="text-xs text-muted-foreground">{f.hint}</span>
              </label>
            ))}
          </div>
        </fieldset>

        <details className="rounded-lg border border-border px-3.5 py-3 text-[13px]">
          <summary className="cursor-pointer font-medium">Save to a folder instead</summary>
          <div className="mt-3 flex flex-col gap-2">
            <label htmlFor="report-folder" className="text-xs text-muted-foreground">
              A new file is created there; existing files are never replaced.
            </label>
            <div className="flex gap-2">
              <Input id="report-folder" className="font-mono" placeholder="D:\Reports" value={folder} onChange={(e) => setFolder(e.target.value)} />
              <Button variant="secondary" disabled={busy || chosen.size === 0 || !folder.trim()} onClick={runSave}>
                <FolderOutput /> {save.isPending ? "Saving…" : "Save"}
              </Button>
            </div>
            {saved && (
              <p role="status" className="m-0 text-xs text-success">
                Saved {saved.file_name} to {saved.path}
              </p>
            )}
          </div>
        </details>

        {busy && (
          <p role="status" className="m-0 text-xs text-muted-foreground">
            Creating the report from stored scan data…
          </p>
        )}
        {error && (
          <div role="alert" className="flex flex-wrap items-center gap-3 rounded-lg bg-danger-soft px-3 py-2.5 text-xs text-danger-soft-foreground">
            <span className="min-w-0 flex-1">{message(error)}</span>
            <Button size="sm" variant="secondary" onClick={last === "download" ? runDownload : runSave} disabled={busy}>
              Retry
            </Button>
          </div>
        )}
      </div>
    </DialogContent>
  );
}
