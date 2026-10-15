import { useMemo } from "react";
import type { ColumnDef } from "@tanstack/react-table";
import { ExternalLink, Package } from "lucide-react";
import { PageHeader } from "@/app/PageHeader";
import { useApps } from "@/api/queries";
import type { AppView } from "@/api/types/AppView";
import type { LastUsedSource } from "@/api/types/LastUsedSource";
import type { SizeSource } from "@/api/types/SizeSource";
import { DataTable } from "@/components/DataTable";
import { RelativeTime, SizeDisplay } from "@/components/SizeDisplay";
import { EmptyState, ErrorState } from "@/components/States";
import { buttonVariants } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";

/** Opened by the browser on the user's click; diskdash starts nothing. */
export const APPS_AND_FEATURES = "ms-settings:appsfeatures";

const SIZE_SOURCE: Record<SizeSource, string> = { estimated: "Registry estimate", scan: "From scan", unknown: "" };
const USED_SOURCE: Record<LastUsedSource, string> = { prefetch: "Prefetch", userassist: "UserAssist", unknown: "" };

function SourceBadge({ label }: { label: string }) {
  if (!label) return null;
  return <span className="mt-0.5 rounded-md bg-muted px-1.5 py-0.5 text-[11px] leading-none font-medium text-muted-foreground">{label}</span>;
}

function AppsAndFeaturesLink({ label }: { label: string }) {
  return (
    <a href={APPS_AND_FEATURES} className={cn(buttonVariants({ variant: "outline", size: "sm" }), "no-underline")} aria-label={label}>
      <ExternalLink /> Apps &amp; features
    </a>
  );
}

const columns: ColumnDef<AppView, unknown>[] = [
  {
    id: "name",
    accessorFn: (a) => a.name,
    header: "Application",
    meta: { width: "minmax(0,2fr)" },
    cell: (c) => {
      const a = c.row.original;
      return (
        <span className="flex min-w-0 flex-col">
          <span className="truncate font-medium" title={a.name}>
            {a.name}
          </span>
          {a.install_location && (
            <span className="truncate font-mono text-[11px] text-subtle-foreground" title={a.install_location}>
              {a.install_location}
            </span>
          )}
        </span>
      );
    },
  },
  { id: "publisher", accessorFn: (a) => a.publisher ?? "", header: "Publisher", meta: { width: "minmax(0,1.2fr)" } },
  { id: "version", accessorFn: (a) => a.version ?? "", header: "Version", meta: { width: "90px" } },
  {
    id: "installed",
    accessorFn: (a) => a.install_date ?? "",
    header: "Installed",
    meta: { width: "100px" },
    cell: (c) => c.row.original.install_date ?? <span className="text-subtle-foreground">unknown</span>,
  },
  {
    id: "size",
    accessorFn: (a) => a.size ?? -1,
    header: "Size",
    meta: { align: "right", width: "130px" },
    cell: (c) => {
      const a = c.row.original;
      return a.size === null ? (
        <span className="text-subtle-foreground">unknown</span>
      ) : (
        <span className="flex flex-col items-end">
          <SizeDisplay bytes={a.size} className="font-semibold" />
          <SourceBadge label={SIZE_SOURCE[a.size_source]} />
        </span>
      );
    },
  },
  {
    id: "lastUsed",
    accessorFn: (a) => a.last_used ?? "",
    header: "Last used",
    meta: { align: "right", width: "120px" },
    cell: (c) => {
      const a = c.row.original;
      return a.last_used === null ? (
        <span className="text-subtle-foreground">unknown</span>
      ) : (
        <span className="flex flex-col items-end">
          <RelativeTime date={a.last_used} />
          <SourceBadge label={USED_SOURCE[a.last_used_source]} />
        </span>
      );
    },
  },
];

export function AppsPage() {
  const apps = useApps();
  const data = apps.data;
  const rows = useMemo(() => data?.apps ?? [], [data]);

  return (
    <>
      <PageHeader
        title="Installed apps"
        subtitle="Read from the Windows registry. diskdash never uninstalls anything: use Windows Settings for that."
        actions={<AppsAndFeaturesLink label="Open Apps & features in Windows Settings" />}
      />
      {apps.isError ? (
        <Card>
          <ErrorState title="Cannot read the installed applications" error={apps.error} onRetry={() => apps.refetch()} />
        </Card>
      ) : !data ? (
        <Card className="flex flex-col gap-3 p-5" aria-label="Loading applications">
          {Array.from({ length: 8 }, (_, i) => (
            <Skeleton key={i} className="h-8 w-full" />
          ))}
        </Card>
      ) : rows.length === 0 ? (
        <Card>
          <EmptyState icon={<Package />} title="No applications found" description="The Windows registry lists no installed applications for this PC." />
        </Card>
      ) : (
        <>
          <Card className="overflow-hidden">
            <DataTable
              ariaLabel="Installed applications"
              columns={columns}
              data={rows}
              getRowId={(a) => `${a.name}|${a.version ?? ""}`}
              initialSorting={[{ id: "size", desc: true }]}
              filterPlaceholder="Filter applications"
              rowHeight={52}
              actionsWidth="150px"
              height={560}
              rowActions={(a) => <AppsAndFeaturesLink label={`Open Apps & features for ${a.name}`} />}
            />
          </Card>
          <div className="flex flex-col gap-1 text-xs text-muted-foreground">
            <p className="m-0">
              Last used is a best guess: it comes from UserAssist (programs you started from Explorer or the Start menu).
              {data.prefetch_available
                ? " Prefetch data is also used."
                : " Prefetch data needs “Enable elevated features”, which is not on."}
            </p>
            {data.skipped > 0 && (
              <p className="m-0 text-warning">
                {data.skipped} registry entr{data.skipped === 1 ? "y" : "ies"} could not be read and {data.skipped === 1 ? "is" : "are"} not listed.
              </p>
            )}
          </div>
        </>
      )}
    </>
  );
}
