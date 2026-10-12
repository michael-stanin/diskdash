import type { ReactNode } from "react";
import type { ColumnDef } from "@tanstack/react-table";
import { Copy, FolderOpen, HardDrive, Inbox, Sparkles } from "lucide-react";
import { PageHeader } from "@/app/PageHeader";
import { MediaBadge, RiskBadge } from "@/components/Badges";
import { DataTable } from "@/components/DataTable";
import { FillBar, fillState } from "@/components/FillBar";
import { RelativeTime, SizeDisplay } from "@/components/SizeDisplay";
import { StatCard, StatCardSkeleton } from "@/components/StatCard";
import { EmptyState, ErrorState } from "@/components/States";
import { useToast } from "@/components/Toast";
import { Button } from "@/components/ui/button";
import { Card, CardBody, CardHeader, CardTitle } from "@/components/ui/card";
import { Input, Select } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { CATEGORY_KEYS, CATEGORY_LABELS } from "@/lib/chart-theme";
import { cn } from "@/lib/cn";

const NINETY_DAYS_AGO = new Date(Date.now() - 90 * 24 * 3600 * 1000);

interface DemoRow {
  path: string;
  size: number;
  files: number;
}

const DEMO_ROWS: DemoRow[] = Array.from({ length: 2000 }, (_, i) => ({
  path: `D:\\Sample\\folder-${String(i).padStart(4, "0")}`,
  size: ((i * 2654435761) % 4_000_000_000) + 1_000_000,
  files: (i * 37) % 9000,
}));

const DEMO_COLUMNS: ColumnDef<DemoRow, unknown>[] = [
  { accessorKey: "path", header: "Path", meta: { mono: true } },
  {
    accessorKey: "size",
    header: "Size",
    meta: { align: "right", width: "96px" },
    cell: (c) => <SizeDisplay bytes={c.getValue<number>()} className="font-semibold" />,
  },
  {
    accessorKey: "files",
    header: "Files",
    meta: { align: "right", width: "84px" },
    cell: (c) => c.getValue<number>().toLocaleString("en-US"),
  },
];

const CAT_BG: Record<(typeof CATEGORY_KEYS)[number], string> = {
  video: "bg-cat-video",
  images: "bg-cat-images",
  audio: "bg-cat-audio",
  archives: "bg-cat-archives",
  installers: "bg-cat-installers",
  documents: "bg-cat-documents",
  code: "bg-cat-code",
  "disk-images": "bg-cat-disk-images",
  system: "bg-cat-system",
  other: "bg-cat-other",
};

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
      </CardHeader>
      <CardBody className="flex flex-wrap items-start gap-4">{children}</CardBody>
    </Card>
  );
}

/** Every shared component in its states. Playwright screenshots this in both themes. */
export function ComponentGallery() {
  const { toast } = useToast();
  const sample = 123456789012;
  return (
    <>
      <PageHeader title="Component gallery" subtitle="Shared components in every state, for both themes." />

      <Section title="Buttons">
        <Button variant="primary">Primary</Button>
        <Button>Secondary</Button>
        <Button variant="outline">Outline</Button>
        <Button variant="ghost">Ghost</Button>
        <Button variant="primary" disabled>
          Disabled
        </Button>
        <Button size="icon" aria-label="Open in Explorer">
          <FolderOpen />
        </Button>
        <Button variant="secondary" onClick={() => toast("Path copied")}>
          <Copy /> Show toast
        </Button>
      </Section>

      <Section title="Stat cards">
        <div className="grid w-full grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
          <StatCard label="Total capacity" value={<SizeDisplay bytes={1.6 * 1024 ** 4} variant="large" />} detail="NVMe · SATA SSD · HDD" />
          <StatCard label="Free space" value={<SizeDisplay bytes={316 * 1024 ** 3} variant="large" />} detail="−18 GiB since last week" />
          <StatCard tone="accent" icon={<Sparkles />} label="Potential savings" value={<SizeDisplay bytes={148 * 1024 ** 3} variant="large" />} detail="Safe 52 · Review 71 · Expert 25" />
          <StatCardSkeleton />
        </div>
      </Section>

      <Section title="Sizes, times, badges">
        <div className="flex flex-col gap-2 text-[13px]">
          <span>
            Size: <SizeDisplay bytes={sample} className="font-semibold" /> (hover for exact bytes)
          </span>
          <span>
            Modified <RelativeTime date={NINETY_DAYS_AGO} />
          </span>
        </div>
        <div className="flex gap-2">
          <MediaBadge media="NVME_SSD" />
          <MediaBadge media="SATA_SSD" />
          <MediaBadge media="HDD" />
          <MediaBadge media="UNKNOWN" />
        </div>
        <div className="flex gap-2">
          <RiskBadge risk="safe" />
          <RiskBadge risk="review" />
          <RiskBadge risk="expert" />
        </div>
      </Section>

      <Section title="Fill bars">
        <div className="flex w-full flex-col gap-3">
          {[
            [0.42, false],
            [0.77, false],
            [0.91, true],
          ].map(([used, low]) => (
            <div key={String(used)} className="flex items-center gap-3">
              <span className="num w-12 text-xs text-muted-foreground">{Math.round((used as number) * 100)}%</span>
              <FillBar
                className="flex-1"
                usedFraction={used as number}
                state={fillState(used as number, low as boolean)}
                label={`${Math.round((used as number) * 100)}% used`}
              />
            </div>
          ))}
        </div>
      </Section>

      <Section title="Category palette">
        {CATEGORY_KEYS.map((k) => (
          <span key={k} className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <span className={cn("size-2.5 rounded-[3px]", CAT_BG[k])} />
            {CATEGORY_LABELS[k]}
          </span>
        ))}
      </Section>

      <Section title="Form controls">
        <label className="flex w-56 flex-col gap-1 text-[13px]">
          Number
          <Input type="number" defaultValue={10} />
        </label>
        <label className="flex w-56 flex-col gap-1 text-[13px]">
          Invalid
          <Input type="number" defaultValue={150} aria-invalid />
        </label>
        <label className="flex w-56 flex-col gap-1 text-[13px]">
          Select
          <Select defaultValue="a">
            <option value="a">Windows drive only</option>
            <option value="b">All drives</option>
          </Select>
        </label>
      </Section>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <Card>
          <EmptyState icon={<Inbox />} title="No scans yet" description="Scan a drive to see where the space goes." action={<Button variant="primary"><HardDrive /> Scan D:</Button>} />
        </Card>
        <Card>
          <ErrorState error={new Error("GET /api/v1/snapshots failed with 500")} onRetry={() => toast("Retried", "info")} />
        </Card>
        <Card>
          <CardBody className="flex flex-col gap-3">
            <Skeleton className="h-5 w-40" />
            <Skeleton className="h-3 w-full" />
            <Skeleton className="h-3 w-5/6" />
            <Skeleton className="h-24 w-full" />
          </CardBody>
        </Card>
      </div>

      <Card className="overflow-hidden">
        <CardHeader>
          <CardTitle>Data table (2,000 rows, virtualised)</CardTitle>
        </CardHeader>
        <DataTable
          ariaLabel="Sample folders"
          columns={DEMO_COLUMNS}
          data={DEMO_ROWS}
          getRowId={(r) => r.path}
          initialSorting={[{ id: "size", desc: true }]}
          height={320}
          rowActions={(r) => (
            <>
              <Button size="icon-sm" variant="outline" aria-label={`Open ${r.path} in Explorer`}>
                <FolderOpen />
              </Button>
              <Button size="icon-sm" variant="outline" aria-label={`Copy ${r.path}`} onClick={() => toast("Path copied")}>
                <Copy />
              </Button>
            </>
          )}
        />
      </Card>
    </>
  );
}
