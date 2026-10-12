import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";
import { AlertTriangle, CheckCircle2, Info, X } from "lucide-react";
import { cn } from "@/lib/cn";

export interface ToastMessage {
  id: number;
  title: string;
  tone: "success" | "info" | "error";
}

interface ToastContextValue {
  toast: (title: string, tone?: ToastMessage["tone"]) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);

/** In-app confirmations ("Path copied"). Never an OS notification. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastMessage[]>([]);
  const nextId = useRef(1);

  const dismiss = useCallback((id: number) => setItems((xs) => xs.filter((x) => x.id !== id)), []);
  const toast = useCallback(
    (title: string, tone: ToastMessage["tone"] = "success") => {
      const id = nextId.current++;
      setItems((xs) => [...xs.slice(-2), { id, title, tone }]);
      setTimeout(() => dismiss(id), 2500);
    },
    [dismiss],
  );
  const value = useMemo(() => ({ toast }), [toast]);

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div aria-live="polite" className="pointer-events-none fixed right-5 bottom-5 z-50 flex flex-col gap-2">
        {items.map((t) => (
          <div
            key={t.id}
            className="pointer-events-auto flex items-center gap-2.5 rounded-xl border border-border bg-card-raised py-2.5 pr-2 pl-3.5 text-[13px] shadow-popover"
          >
            <span
              className={cn(
                "[&_svg]:size-4",
                t.tone === "success" ? "text-success" : t.tone === "error" ? "text-danger" : "text-primary",
              )}
            >
              {t.tone === "success" ? <CheckCircle2 /> : t.tone === "error" ? <AlertTriangle /> : <Info />}
            </span>
            {t.title}
            <button
              type="button"
              aria-label="Dismiss"
              onClick={() => dismiss(t.id)}
              className="ml-1 rounded-md p-1 text-subtle-foreground hover:bg-hover hover:text-foreground"
            >
              <X className="size-3.5" />
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextValue {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToast must be used inside ToastProvider");
  return ctx;
}
