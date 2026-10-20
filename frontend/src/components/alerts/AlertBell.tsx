import { Bell } from "lucide-react";
import { Link } from "react-router";
import { useAlertList } from "@/api/queries";
import { Tooltip } from "@/components/ui/tooltip";
import { cn } from "@/lib/cn";

/** Header bell: the number of unresolved alerts, or "?" when they cannot be loaded (never a false zero). */
export function AlertBell() {
  const alerts = useAlertList("active");
  const unknown = alerts.isError;
  const count = alerts.data?.active ?? 0;
  const label = unknown ? "Alerts: state unknown (could not be loaded)" : count === 0 ? "Alerts: none" : `Alerts: ${count} active`;
  return (
    <Tooltip content={label}>
      <Link
        to="/alerts"
        aria-label={label}
        className="relative inline-flex size-9 items-center justify-center rounded-lg border border-border-strong bg-card text-foreground-secondary no-underline transition-colors hover:bg-hover"
      >
        <Bell className="size-4" />
        {(unknown || count > 0) && (
          <span
            className={cn(
              "num absolute -top-1.5 -right-1.5 flex min-w-[18px] items-center justify-center rounded-full px-1 text-[11px] leading-[18px] font-semibold",
              unknown ? "bg-muted text-foreground-secondary" : "bg-danger text-primary-foreground",
            )}
          >
            {unknown ? "?" : count > 99 ? "99+" : count}
          </span>
        )}
      </Link>
    </Tooltip>
  );
}
