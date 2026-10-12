import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import type { Measure } from "@/api/types/Measure";
import { cn } from "@/lib/cn";

const KEY = "diskdash.sizeMode";

function read(): Measure {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "alloc" || v === "size") return v;
  } catch {
    // Storage unavailable: default.
  }
  return "alloc";
}

interface SizeModeValue {
  measure: Measure;
  setMeasure: (m: Measure) => void;
}

const SizeModeContext = createContext<SizeModeValue | null>(null);

/** One global "size on disk" / "logical size" choice, remembered across reloads. */
export function SizeModeProvider({ children }: { children: ReactNode }) {
  const [measure, setState] = useState<Measure>(read);
  const setMeasure = useCallback((m: Measure) => {
    setState(m);
    try {
      localStorage.setItem(KEY, m);
    } catch {
      // Not persisted.
    }
  }, []);
  const value = useMemo(() => ({ measure, setMeasure }), [measure, setMeasure]);
  return <SizeModeContext.Provider value={value}>{children}</SizeModeContext.Provider>;
}

export function useSizeMode(): SizeModeValue {
  const ctx = useContext(SizeModeContext);
  if (!ctx) throw new Error("useSizeMode must be used inside SizeModeProvider");
  return ctx;
}

export function sizeOf(e: { size: number; alloc: number }, m: Measure): number {
  return m === "alloc" ? e.alloc : e.size;
}

export function SizeModeToggle({ className }: { className?: string }) {
  const { measure, setMeasure } = useSizeMode();
  return (
    <div role="group" aria-label="Size measure" className={cn("flex overflow-hidden rounded-lg border border-border-strong text-xs", className)}>
      {(["alloc", "size"] as const).map((m) => (
        <button
          key={m}
          type="button"
          aria-pressed={measure === m}
          onClick={() => setMeasure(m)}
          className={cn("h-8 px-3", measure === m ? "bg-muted font-medium text-foreground" : "text-muted-foreground hover:bg-hover")}
        >
          {m === "alloc" ? "Size on disk" : "Logical size"}
        </button>
      ))}
    </div>
  );
}
