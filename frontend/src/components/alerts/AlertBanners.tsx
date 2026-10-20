import { AlertTriangle, X } from "lucide-react";
import { Link } from "react-router";
import { useAlertList, useDismissAlert } from "@/api/queries";
import { cn } from "@/lib/cn";

/**
 * Dismissible banners for unresolved low-space alerts. Dismissing hides a banner until the alert is raised
 * again; it never changes anything on disk. On the drive view pass that drive's `volumeId`.
 */
export function AlertBanners({ volumeId }: { volumeId?: string }) {
  const alerts = useAlertList("active");
  const dismiss = useDismissAlert();
  const shown = (alerts.data?.alerts ?? []).filter((a) => a.kind === "low_space" && !a.dismissed && (!volumeId || a.volume_id === volumeId));
  if (shown.length === 0) return null;
  return (
    <div className="flex flex-col gap-2" aria-label="Alerts">
      {shown.map((a) => (
        <div
          key={a.id}
          role="alert"
          className={cn(
            "flex flex-wrap items-center gap-x-3 gap-y-1 rounded-[10px] border px-4 py-3 text-[13px]",
            a.severity === "critical"
              ? "border-danger-soft-border bg-danger-soft text-danger-soft-foreground"
              : "border-risk-review-fg/20 bg-risk-review-bg text-risk-review-fg",
          )}
        >
          <AlertTriangle className="size-[18px] shrink-0" aria-hidden />
          <span className="min-w-0 flex-1">
            <strong className="font-semibold">{a.title}</strong> — {a.message}
          </span>
          <span className="flex items-center gap-3">
            <Link to="/recommendations" className="font-medium whitespace-nowrap text-primary no-underline hover:text-primary-hover">
              View suggestions
            </Link>
            {a.volume_id && !volumeId && (
              <Link to={`/drives/${a.volume_id}`} className="font-medium whitespace-nowrap text-primary no-underline hover:text-primary-hover">
                Review
              </Link>
            )}
            <button
              type="button"
              aria-label={`Dismiss: ${a.title}`}
              disabled={dismiss.isPending}
              onClick={() => dismiss.mutate(a.id)}
              className="rounded-md p-1 hover:bg-hover/60"
            >
              <X className="size-3.5" />
            </button>
          </span>
        </div>
      ))}
    </div>
  );
}
