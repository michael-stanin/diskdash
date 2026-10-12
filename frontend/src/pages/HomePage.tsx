import { HardDrive, ShieldCheck } from "lucide-react";
import { PageHeader } from "@/app/PageHeader";
import { useHealth, useSystem } from "@/api/queries";
import { EmptyState, ErrorState } from "@/components/States";
import { StatCard, StatCardSkeleton } from "@/components/StatCard";
import { Card } from "@/components/ui/card";

export function HomePage() {
  const system = useSystem();
  const health = useHealth();

  return (
    <>
      <PageHeader
        title="Overview"
        subtitle="Read-only: diskdash shows and suggests, it never changes anything on disk."
      />

      <section aria-label="Summary" className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
        {system.isPending ? (
          Array.from({ length: 4 }, (_, i) => <StatCardSkeleton key={i} />)
        ) : system.isError ? (
          <Card className="sm:col-span-2 xl:col-span-4">
            <ErrorState title="Cannot load system information" error={system.error} onRetry={() => system.refetch()} />
          </Card>
        ) : (
          <>
            <StatCard label="Server" value={health.isSuccess ? "Running" : "—"} detail={`version ${system.data.version}`} />
            <StatCard
              label="Mode"
              value={system.data.elevated ? "Administrator" : "Standard"}
              detail={system.data.elevated ? "All features available" : `${system.data.reduced_features.length} features need admin`}
            />
            <StatCard
              label="Windows"
              value={system.data.os.display_version ?? "—"}
              detail={system.data.os.build ? `build ${system.data.os.build}` : (system.data.os.product ?? "")}
            />
            <StatCard
              tone="accent"
              icon={<ShieldCheck />}
              label="Safety"
              value="Read-only"
              detail="No file is ever modified"
            />
          </>
        )}
      </section>

      <Card>
        <EmptyState
          icon={<HardDrive />}
          title="Your drives will appear here"
          description="Drive cards with free space, hardware type and health come with the drive inventory, followed by scans of what fills each drive."
        />
      </Card>
    </>
  );
}
