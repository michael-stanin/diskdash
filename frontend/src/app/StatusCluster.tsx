import { Laptop, Loader2, Moon, ShieldAlert, Sun } from "lucide-react";
import { useEvents } from "@/api/events";
import { useSystem } from "@/api/queries";
import { AlertBell } from "@/components/alerts/AlertBell";
import { Button } from "@/components/ui/button";
import { Tooltip } from "@/components/ui/tooltip";
import { cn } from "@/lib/cn";
import { useTheme, type ThemePreference } from "@/lib/theme";

const CONNECTION = {
  connected: { label: "Connected", dot: "bg-success" },
  connecting: { label: "Connecting…", dot: "bg-warning" },
  disconnected: { label: "Disconnected", dot: "bg-danger" },
} as const;

export function ConnectionStatus() {
  const { state } = useEvents();
  const c = CONNECTION[state];
  return (
    <span role="status" className="flex items-center gap-1.5 text-xs text-muted-foreground">
      <span className={cn("size-[7px] rounded-full", c.dot)} />
      {c.label}
    </span>
  );
}

export function ScanIndicator() {
  const { activeJobs } = useEvents();
  const running = activeJobs.filter((j) => j.status === "running");
  if (running.length === 0) return null;
  const job = running[0]!;
  const pct = job.total ? Math.round((job.done / job.total) * 100) : null;
  return (
    <span className="flex items-center gap-1.5 rounded-full bg-primary-soft px-2.5 py-1 text-xs text-primary-soft-foreground">
      <Loader2 className="size-3.5 animate-spin" />
      {running.length > 1 ? `${running.length} tasks running` : (job.phase ?? job.kind)}
      {pct !== null && <span className="num">{pct}%</span>}
    </span>
  );
}

export function ReducedModeBadge() {
  const system = useSystem();
  if (!system.data || system.data.elevated) return null;
  return (
    <Tooltip
      content={
        <div className="flex flex-col gap-1">
          <span className="font-medium">Running without administrator rights.</span>
          <span className="text-muted-foreground">Not available in this mode:</span>
          <ul className="m-0 list-disc pl-4 text-muted-foreground">
            {system.data.reduced_features.map((f) => (
              <li key={f.id}>{f.label}</li>
            ))}
          </ul>
        </div>
      }
    >
      <button
        type="button"
        className="flex items-center gap-1.5 rounded-full border border-border px-2.5 py-1 text-xs text-warning"
      >
        <ShieldAlert className="size-3.5" /> Reduced mode
      </button>
    </Tooltip>
  );
}

const NEXT: Record<ThemePreference, ThemePreference> = { system: "light", light: "dark", dark: "system" };
const THEME_ICON = { system: Laptop, light: Sun, dark: Moon } as const;

export function ThemeToggle() {
  const { preference, setPreference } = useTheme();
  const Icon = THEME_ICON[preference];
  return (
    <Tooltip content={`Theme: ${preference} (click to change)`}>
      <Button
        variant="secondary"
        size="icon"
        aria-label={`Theme: ${preference}. Switch to ${NEXT[preference]}`}
        onClick={() => setPreference(NEXT[preference])}
      >
        <Icon />
      </Button>
    </Tooltip>
  );
}

/** Right side of every page header. */
export function StatusCluster() {
  return (
    <div className="flex items-center gap-3">
      <ScanIndicator />
      <ReducedModeBadge />
      <ConnectionStatus />
      <AlertBell />
      <ThemeToggle />
    </div>
  );
}
