import { lazy, Suspense } from "react";
import type { Snapshot } from "@/api/types/Snapshot";
import { Skeleton } from "@/components/ui/skeleton";

// ECharts is large; load it only when a usage view is opened.
const UsageExplorer = lazy(() => import("./UsageExplorer").then((m) => ({ default: m.UsageExplorer })));

export function LazyUsageExplorer({ snapshot }: { snapshot: Snapshot }) {
  return (
    <Suspense fallback={<Skeleton className="h-[440px] w-full rounded-card" />}>
      <UsageExplorer key={snapshot.id} snapshot={snapshot} />
    </Suspense>
  );
}
