import { useState, type ReactNode } from "react";
import { Save, Undo2 } from "lucide-react";
import { PageHeader } from "@/app/PageHeader";
import { ApiError } from "@/api/client";
import { useSaveSettings, useSettings } from "@/api/queries";
import type { HardlinkDefault } from "@/api/types/HardlinkDefault";
import type { Settings } from "@/api/types/Settings";
import { ErrorState } from "@/components/States";
import { useToast } from "@/components/Toast";
import { Button } from "@/components/ui/button";
import { Card, CardBody, CardHeader, CardTitle } from "@/components/ui/card";
import { Input, Select } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { PathList } from "@/components/settings/PathList";

type NumericKey = {
  [K in keyof Settings]: Settings[K] extends number ? K : never;
}[keyof Settings];

interface FieldProps {
  id: string;
  label: string;
  hint: ReactNode;
  error?: string;
  children: ReactNode;
}

function Field({ id, label, hint, error, children }: FieldProps) {
  return (
    <div className="grid grid-cols-1 gap-2 py-3 md:grid-cols-[minmax(0,1fr)_200px] md:items-center md:gap-6">
      <div className="flex flex-col gap-0.5">
        <label htmlFor={id} className="text-[13px] font-medium">
          {label}
        </label>
        <span id={`${id}-hint`} className="text-xs text-muted-foreground">
          {hint}
        </span>
      </div>
      <div className="flex flex-col gap-1">
        {children}
        {error && (
          <span id={`${id}-error`} role="alert" className="text-xs text-danger">
            {error}
          </span>
        )}
      </div>
    </div>
  );
}

export function SettingsPage() {
  const settings = useSettings();
  if (settings.isError) {
    return (
      <>
        <PageHeader title="Settings" />
        <Card>
          <ErrorState title="Cannot load settings" error={settings.error} onRetry={() => settings.refetch()} />
        </Card>
      </>
    );
  }
  if (!settings.data) return <SettingsSkeleton />;
  // Re-key on the saved values so the form starts from them after every save.
  return <SettingsForm key={JSON.stringify(settings.data)} saved={settings.data} />;
}

function SettingsSkeleton() {
  return (
    <>
      <PageHeader title="Settings" />
      <Card>
        <CardBody className="flex flex-col gap-5">
          {Array.from({ length: 6 }, (_, i) => (
            <div key={i} className="flex items-center justify-between gap-6">
              <div className="flex flex-col gap-2">
                <Skeleton className="h-4 w-48" />
                <Skeleton className="h-3 w-72" />
              </div>
              <Skeleton className="h-9 w-[200px]" />
            </div>
          ))}
        </CardBody>
      </Card>
    </>
  );
}

function SettingsForm({ saved }: { saved: Settings }) {
  const save = useSaveSettings();
  const { toast } = useToast();
  const [draft, setDraft] = useState<Settings>(saved);

  const fieldErrors = save.error instanceof ApiError ? save.error.fields : {};
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);

  const num = (key: NumericKey, label: string, hint: string, unit: string, step = 1) => (
    <Field id={key} label={label} hint={hint} error={fieldErrors[key]}>
      <div className="flex items-center gap-2">
        <Input
          id={key}
          type="number"
          step={step}
          value={Number.isNaN(draft[key]) ? "" : draft[key]}
          aria-invalid={Boolean(fieldErrors[key])}
          aria-describedby={`${key}-hint${fieldErrors[key] ? ` ${key}-error` : ""}`}
          onChange={(e) => setDraft((d) => ({ ...d, [key]: e.target.valueAsNumber }))}
        />
        <span className="w-16 shrink-0 text-xs text-muted-foreground">{unit}</span>
      </div>
    </Field>
  );

  const onSave = () => {
    save.mutate(draft, { onSuccess: () => toast("Settings saved") });
  };

  return (
    <>
      <PageHeader
        title="Settings"
        subtitle="Thresholds and scan behaviour. Nothing here can make diskdash change your files."
        actions={
          <>
            <Button variant="ghost" disabled={!dirty || save.isPending} onClick={() => setDraft(saved)}>
              <Undo2 /> Reset
            </Button>
            <Button variant="primary" disabled={!dirty || save.isPending} onClick={onSave}>
              <Save /> {save.isPending ? "Saving…" : "Save"}
            </Button>
          </>
        }
      />

      <div className="flex flex-col gap-4">
          {save.isError && !(save.error instanceof ApiError && save.error.status === 422) && (
            <Card>
              <ErrorState title="Settings were not saved" error={save.error} onRetry={onSave} />
            </Card>
          )}
          <Card>
            <CardHeader>
              <CardTitle>Low space warning</CardTitle>
            </CardHeader>
            <CardBody className="divide-y divide-border py-2">
              {num("low_space_percent", "Percentage threshold", "Warn when free space drops below this share of the drive.", "% free", 0.5)}
              {num("low_space_floor_gib", "Absolute floor", "…or when less than this much is free, whichever comes first.", "GiB free")}
            </CardBody>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle>Analysis</CardTitle>
            </CardHeader>
            <CardBody className="divide-y divide-border py-2">
              {num("stale_months", "Stale after", "Files untouched for this long count as stale.", "months")}
              {num("tiering_cold_months", "Cold on SSD after", "Tiering: data on an SSD that nobody modified for this long is suggested for an HDD.", "months")}
              {num("tiering_min_gib", "Smallest move suggested", "Tiering: smaller folders and files are not worth moving.", "GiB")}
              {num("tiering_hot_days", "Hot on HDD within", "Tiering: data on an HDD modified within this many days is suggested for an SSD.", "days")}
              {num("retention_keep", "Full scans kept", "History: how many full scans to keep per folder. Older ones shrink to folder totals so history stays small.", "scans")}
              {num("retention_depth", "Folder depth kept", "History: how many folder levels an older scan keeps after it shrinks.", "levels")}
              {num("chart_children_per_level", "Chart detail", "Folders shown per level in the treemap before grouping into “Other”.", "per level")}
            </CardBody>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle>Exclusions and protected folders</CardTitle>
            </CardHeader>
            <CardBody className="divide-y divide-border py-2">
              <PathList
                id="excluded_paths"
                label="Excluded folders"
                hint="Nothing in these folders is ever suggested: not as cleanup, stale data, duplicates or a move."
                value={draft.excluded_paths}
                error={fieldErrors.excluded_paths}
                emptyText="No exclusions yet. Add a folder you never want to see in suggestions, such as a photo archive."
                onChange={(v) => setDraft((d) => ({ ...d, excluded_paths: v }))}
              />
              <PathList
                id="protected_paths"
                label="Protected folders"
                hint="Like exclusions, and they can never be added to a plan. Windows and Program Files are always protected."
                value={draft.protected_paths}
                error={fieldErrors.protected_paths}
                emptyText="No extra protected folders. The built-in protection (Windows, Program Files and similar) is always on."
                onChange={(v) => setDraft((d) => ({ ...d, protected_paths: v }))}
              />
            </CardBody>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle>Scanning</CardTitle>
            </CardHeader>
            <CardBody className="divide-y divide-border py-2">
              {num("hdd_scan_concurrency", "HDD threads", "Hard disks are read sequentially to avoid head thrashing.", "per disk")}
              {num("ssd_scan_concurrency", "SSD threads", "SSDs can be scanned in parallel.", "per disk")}
              <Field
                id="hardlink_detection"
                label="Hard-link detection"
                hint="Counting hard-linked files once costs one extra lookup per file. Needed mainly on the Windows drive."
                error={fieldErrors.hardlink_detection}
              >
                <Select
                  id="hardlink_detection"
                  value={draft.hardlink_detection}
                  onChange={(e) =>
                    setDraft((d) => ({ ...d, hardlink_detection: e.target.value as HardlinkDefault }))
                  }
                >
                  <option value="system-volume">Windows drive only</option>
                  <option value="all-volumes">All drives</option>
                  <option value="no-volumes">Off</option>
                </Select>
              </Field>
            </CardBody>
          </Card>
      </div>
    </>
  );
}
