import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { JobStatus } from "./types/JobStatus";
import type { ServerEvent } from "./types/ServerEvent";
import { cleanupKeys, keys, useHealth } from "./queries";

export type ConnectionState = "connecting" | "connected" | "disconnected";

const EVENT_TYPES = ["job.queued", "job.started", "job.progress", "job.finished", "inventory.updated"] as const;
type Listener = (e: ServerEvent) => void;

interface EventsContextValue {
  state: ConnectionState;
  subscribe: (fn: Listener) => () => void;
  activeJobs: JobStatus[];
}

const EventsContext = createContext<EventsContextValue | null>(null);

/** Reconnect delay with backoff: 1 s, 2 s, 4 s … capped at 15 s. */
export function backoff(attempt: number): number {
  return Math.min(15_000, 1_000 * 2 ** Math.max(0, attempt));
}

/**
 * One EventSource for the whole app. The browser resends Last-Event-ID on its
 * own reconnects; we rebuild the source after hard failures with backoff.
 */
export function EventsProvider({ children, url = "/api/v1/events" }: { children: ReactNode; url?: string }) {
  const [sseOpen, setSseOpen] = useState(false);
  const [activeJobs, setActiveJobs] = useState<Record<number, JobStatus>>({});
  const listeners = useRef(new Set<Listener>());
  const qc = useQueryClient();
  const health = useHealth();

  useEffect(() => {
    let source: EventSource | null = null;
    let attempt = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let closed = false;

    const onEvent = (msg: MessageEvent<string>) => {
      let event: ServerEvent;
      try {
        event = JSON.parse(msg.data) as ServerEvent;
      } catch {
        return;
      }
      if (event.type === "inventory.updated") void qc.invalidateQueries({ queryKey: keys.drives });
      const job = event.data as JobStatus;
      if (event.type.startsWith("job.") && job && typeof job.id === "number") {
        setActiveJobs((prev) => {
          const next = { ...prev };
          if (event.type === "job.finished") delete next[job.id];
          else next[job.id] = job;
          return next;
        });
        if (event.type === "job.finished") {
          void qc.invalidateQueries({ queryKey: keys.jobs });
          if (job.kind === "scan") void qc.invalidateQueries({ queryKey: keys.snapshots });
          if (job.kind === "scan" || job.kind === "cleanup") void qc.invalidateQueries({ queryKey: cleanupKeys.all });
          if (job.kind === "duplicates") void qc.invalidateQueries({ queryKey: ["duplicates"] });
          if (job.kind === "verify-plans" || job.kind === "scan") void qc.invalidateQueries({ queryKey: ["plans"] });
          if (job.kind === "scan") void qc.invalidateQueries({ queryKey: ["tiering"] });
          if (job.kind === "scan") void qc.invalidateQueries({ queryKey: ["history"] });
        }
        if (job.kind === "duplicates" && (event.type === "job.queued" || event.type === "job.started")) {
          void qc.invalidateQueries({ queryKey: ["duplicates"] });
        }
        if (job.kind === "cleanup" && (event.type === "job.queued" || event.type === "job.started")) {
          void qc.invalidateQueries({ queryKey: cleanupKeys.all });
        }
        if (event.type === "job.started" && job.kind === "scan") void qc.invalidateQueries({ queryKey: keys.snapshots });
      }
      listeners.current.forEach((fn) => fn(event));
    };

    const connect = () => {
      if (closed) return;
      source = new EventSource(url);
      source.onopen = () => {
        attempt = 0;
        setSseOpen(true);
      };
      source.onerror = () => {
        setSseOpen(false);
        if (source?.readyState === EventSource.CLOSED) {
          source.close();
          timer = setTimeout(connect, backoff(attempt++));
        }
      };
      EVENT_TYPES.forEach((t) => source?.addEventListener(t, onEvent as EventListener));
    };
    connect();
    return () => {
      closed = true;
      clearTimeout(timer);
      source?.close();
    };
  }, [url, qc]);

  // Health polls every 4 s, so a dead backend shows "disconnected" well within 10 s.
  const state: ConnectionState = health.isError ? "disconnected" : health.isSuccess && sseOpen ? "connected" : "connecting";

  const value = useMemo<EventsContextValue>(
    () => ({
      state,
      activeJobs: Object.values(activeJobs),
      subscribe: (fn) => {
        listeners.current.add(fn);
        return () => listeners.current.delete(fn);
      },
    }),
    [state, activeJobs],
  );
  return <EventsContext.Provider value={value}>{children}</EventsContext.Provider>;
}

export function useEvents(): EventsContextValue {
  const ctx = useContext(EventsContext);
  if (!ctx) throw new Error("useEvents must be used inside EventsProvider");
  return ctx;
}

export function useConnectionState(): ConnectionState {
  return useEvents().state;
}
