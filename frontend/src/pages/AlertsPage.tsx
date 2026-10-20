import { useState } from "react";
import { BellRing, CheckCircle2, GitCompareArrows, HardDrive, Settings as SettingsIcon, TrendingUp } from "lucide-react";
import { Link } from "react-router";
import { PageHeader } from "@/app/PageHeader";
import { useAlertList, useDismissAlert, type AlertStatus } from "@/api/queries";
import type { AlertView } from "@/api/types/AlertView";
import { RelativeTime } from "@/components/SizeDisplay";
import { EmptyState, ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";
import { formatAbsolute } from "@/lib/format";

const FILTERS: { key: AlertStatus; label: string }[] = [
  { key: "active", label: "Active" },
  { key: "resolved", label: "Resolved" },
  { key: "all", label: "All" },
];

function Row({ a }: { a: AlertView }) {
  const dismiss = useDismissAlert();
  const active = a.resolved_at === null;
  return (
    <li className="flex flex-col gap-1.5 px-5 py-3.5 text-[13px]">
      <div className="flex flex-wrap items-center gap-2.5">
        <span className="text-muted-foreground [&_svg]:size-4">{a.kind === "low_space" ? <HardDrive aria-hidden /> : <TrendingUp aria-hidden />}</span>
        <span className="font-semibold">{a.title}</span>
        <span
          className={cn(
            "rounded-md px-2 py-0.5 text-[11px] font-semibold",
            a.severity === "critical" ? "bg-risk-expert-bg text-risk-expert-fg" : "bg-risk-review-bg text-risk-review-fg",
          )}
        >
          {a.severity === "critical" ? "Critical" : "Warning"}
        </span>
        {!active && <span className="rounded-md bg-risk-safe-bg px-2 py-0.5 text-[11px] font-semibold text-risk-safe-fg">Resolved</span>}
        {active && a.dismissed && <span className="rounded-md bg-muted px-2 py-0.5 text-[11px] font-semibold text-foreground-secondary">Dismissed</span>}
        <span className="ml-auto text-xs text-muted-foreground">
          <RelativeTime date={a.raised_at} />
        </span>
      </div>
      <p className="m-0 text-muted-foreground">{a.message}</p>
      <div className="flex flex-wrap items-center gap-3 text-xs">
        {a.volume_id && (
          <Link to={`/drives/${a.volume_id}`} className="font-medium text-primary no-underline hover:text-primary-hover">
            Open drive
          </Link>
        )}
        {a.kind === "low_space" && (
          <Link to="/recommendations" className="font-medium text-primary no-underline hover:text-primary-hover">
            View suggestions
          </Link>
        )}
        {a.history && (
          <Link
            to={`/history?old=${a.history.old}&new=${a.history.new}`}
            className="inline-flex items-center gap-1 font-medium text-primary no-underline hover:text-primary-hover"
          >
            <GitCompareArrows className="size-3.5" aria-hidden /> Compare the scans
          </Link>
        )}
        {a.resolved_at && <span className="text-muted-foreground">Resolved {formatAbsolute(a.resolved_at)}</span>}
        {active && !a.dismissed && (
          <Button size="sm" variant="ghost" className="ml-auto" disabled={dismiss.isPending} onClick={() => dismiss.mutate(a.id)}>
            Dismiss banner
          </Button>
        )}
      </div>
    </li>
  );
}

export function AlertsPage() {
  const [status, setStatus] = useState<AlertStatus>("active");
  const q = useAlertList(status);
  const t = q.data?.thresholds;
  return (
    <>
      <PageHeader
        title="Alerts"
        subtitle="Low free space and folders that grow fast. They show up here and in the header; diskdash never sends Windows notifications."
        actions={
          <Button asChild variant="secondary">
            <Link to="/settings">
              <SettingsIcon /> Thresholds
            </Link>
          </Button>
        }
      />
      <p className="m-0 rounded-lg bg-muted px-3.5 py-2.5 text-xs text-muted-foreground">
        Alerts are only checked while diskdash is running
        {q.data && (
          <>
            {" "}
            (this session started <RelativeTime date={q.data.running_since} />)
          </>
        )}
        : after each drive refresh and each finished scan. Nothing is checked while the app is closed.
      </p>
      <div role="group" aria-label="Show" className="flex gap-1.5">
        {FILTERS.map((f) => (
          <Button key={f.key} size="sm" variant={status === f.key ? "primary" : "secondary"} aria-pressed={status === f.key} onClick={() => setStatus(f.key)}>
            {f.label}
          </Button>
        ))}
      </div>
      <Card className="overflow-hidden">
        <CardHeader>
          <BellRing className="size-4 text-muted-foreground" aria-hidden />
          <CardTitle>{FILTERS.find((f) => f.key === status)?.label} alerts</CardTitle>
          {q.data && <span className="num ml-auto text-xs text-muted-foreground">{q.data.alerts.length} shown</span>}
        </CardHeader>
        {q.isPending ? (
          <div className="flex flex-col gap-3 p-5" aria-busy="true">
            {Array.from({ length: 3 }, (_, i) => (
              <Skeleton key={i} className="h-14 w-full" />
            ))}
          </div>
        ) : q.isError ? (
          <ErrorState title="Could not load alerts" error={q.error} onRetry={() => void q.refetch()} />
        ) : q.data.alerts.length === 0 ? (
          <EmptyState
            icon={<CheckCircle2 />}
            title={status === "resolved" ? "No resolved alerts yet" : "All drives look healthy"}
            description={
              t ? (
                <>
                  An alert is raised when a drive has less than {t.low_percent}% or {t.low_floor_gib} GiB free, or a folder grows by more than {t.growth_gib_per_day} GiB a day.{" "}
                  <Link to="/settings" className="font-medium text-primary no-underline hover:text-primary-hover">
                    Change these limits
                  </Link>
                </>
              ) : undefined
            }
          />
        ) : (
          <ul className="m-0 list-none divide-y divide-border p-0" aria-label="Alerts">
            {q.data.alerts.map((a) => (
              <Row key={a.id} a={a} />
            ))}
          </ul>
        )}
      </Card>
    </>
  );
}
