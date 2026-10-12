import { useParams } from "react-router";
import { History, Play } from "lucide-react";
import { PageHeader } from "@/app/PageHeader";
import { ApiError } from "@/api/client";
import { useResumeScan, useSnapshot } from "@/api/queries";
import { ScanActivity } from "@/components/scans/ScanActivity";
import { EmptyState, ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { SnapshotSummary } from "@/components/usage/SnapshotSummary";
import { LazyUsageExplorer } from "@/components/usage/LazyUsageExplorer";

/** Explore any one scan (drive or folder) by id. */
export function SnapshotPage() {
  const { id = "" } = useParams();
  const snapshotId = Number(id);
  const snap = useSnapshot(snapshotId);
  const resume = useResumeScan();
  const s = snap.data;

  const title = s ? s.root : <Skeleton className="h-8 w-48" />;
  const breadcrumb = [{ label: "Drives", to: "/drives" }, { label: s ? `Scan of ${s.root}` : "Scan" }];

  if (snap.isError) {
    const notFound = snap.error instanceof ApiError && snap.error.status === 404;
    return (
      <>
        <PageHeader title="Scan" breadcrumb={breadcrumb} />
        <Card>
          {notFound ? (
            <EmptyState icon={<History />} title="This scan no longer exists" description="It may have been removed." />
          ) : (
            <ErrorState error={snap.error} onRetry={() => snap.refetch()} />
          )}
        </Card>
      </>
    );
  }

  return (
    <>
      <PageHeader title={title} breadcrumb={breadcrumb} />
      <ScanActivity />
      {!s ? (
        <Skeleton className="h-[440px] w-full rounded-card" />
      ) : s.status === "complete" ? (
        <>
          <SnapshotSummary snapshot={s} />
          <LazyUsageExplorer snapshot={s} />
        </>
      ) : (
        <Card>
          <EmptyState
            icon={<History />}
            title={s.status === "running" ? "This scan is still running" : s.status === "partial" ? "This scan was stopped" : "This scan failed"}
            description={s.error ?? undefined}
            action={
              s.status === "partial" && (
                <Button variant="primary" disabled={resume.isPending} onClick={() => resume.mutate(s.id)}>
                  <Play /> Resume scan
                </Button>
              )
            }
          />
        </Card>
      )}
    </>
  );
}
