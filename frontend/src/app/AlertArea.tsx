import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import { AlertTriangle, WifiOff, X } from "lucide-react";
import { useEvents } from "@/api/events";
import { cn } from "@/lib/cn";

export interface AppAlert {
  id: string;
  tone: "warning" | "danger";
  title: string;
  body?: ReactNode;
  action?: ReactNode;
  dismissible?: boolean;
}

interface AlertsContextValue {
  alerts: AppAlert[];
  setAlert: (a: AppAlert) => void;
  clearAlert: (id: string) => void;
}

const AlertsContext = createContext<AlertsContextValue | null>(null);

/** In-app alerts (low space, connection loss…). Never OS notifications. */
export function AlertsProvider({ children }: { children: ReactNode }) {
  const [alerts, setAlerts] = useState<AppAlert[]>([]);
  const setAlert = useCallback((a: AppAlert) => setAlerts((xs) => [...xs.filter((x) => x.id !== a.id), a]), []);
  const clearAlert = useCallback((id: string) => setAlerts((xs) => xs.filter((x) => x.id !== id)), []);
  const value = useMemo(() => ({ alerts, setAlert, clearAlert }), [alerts, setAlert, clearAlert]);
  return <AlertsContext.Provider value={value}>{children}</AlertsContext.Provider>;
}

export function useAlerts(): AlertsContextValue {
  const ctx = useContext(AlertsContext);
  if (!ctx) throw new Error("useAlerts must be used inside AlertsProvider");
  return ctx;
}

function Banner({ alert, onDismiss }: { alert: AppAlert; onDismiss?: () => void }) {
  return (
    <div
      role="alert"
      className={cn(
        "flex items-center gap-3 rounded-[10px] border px-4 py-3 text-[13px]",
        "border-danger-soft-border bg-danger-soft text-danger-soft-foreground",
      )}
    >
      {alert.id === "connection" ? (
        <WifiOff className="size-[18px] shrink-0 text-danger" />
      ) : (
        <AlertTriangle className="size-[18px] shrink-0 text-danger" />
      )}
      <span>
        <strong className="font-semibold">{alert.title}</strong>
        {alert.body && <> — {alert.body}</>}
      </span>
      <span className="ml-auto flex items-center gap-2">
        {alert.action}
        {onDismiss && (
          <button
            type="button"
            aria-label="Dismiss alert"
            onClick={onDismiss}
            className="rounded-md p-1 hover:bg-hover/60"
          >
            <X className="size-3.5" />
          </button>
        )}
      </span>
    </div>
  );
}

export function AlertArea() {
  const { alerts, clearAlert } = useAlerts();
  const { state } = useEvents();
  // Low-space banners come from the recorded alerts (AlertBanners): they can be dismissed and are not repeated.
  const shown: AppAlert[] = [...alerts];
  if (state === "disconnected") {
    shown.unshift({
      id: "connection",
      tone: "danger",
      title: "Disconnected from diskdash",
      body: "the local server is not responding. Data on screen may be out of date; it will reconnect automatically.",
    });
  }
  if (shown.length === 0) return null;
  return (
    <div className="flex flex-col gap-2">
      {shown.map((a) => (
        <Banner key={a.id} alert={a} onDismiss={a.dismissible ? () => clearAlert(a.id) : undefined} />
      ))}
    </div>
  );
}
