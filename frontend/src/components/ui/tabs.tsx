import { useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import { cn } from "@/lib/cn";

export interface TabDef<K extends string> {
  key: K;
  label: ReactNode;
}

interface TabsProps<K extends string> {
  tabs: TabDef<K>[];
  value: K;
  onChange: (k: K) => void;
  label: string;
  children: ReactNode;
}

/** Underline tabs with arrow-key navigation (WAI-ARIA tabs pattern). */
export function Tabs<K extends string>({ tabs, value, onChange, label, children }: TabsProps<K>) {
  const id = useId();
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const onKey = (e: KeyboardEvent, i: number) => {
    const delta = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!delta) return;
    e.preventDefault();
    const next = (i + delta + tabs.length) % tabs.length;
    onChange(tabs[next]!.key);
    refs.current[next]?.focus();
  };
  return (
    <div className="flex flex-col gap-5">
      <div role="tablist" aria-label={label} className="flex gap-1 border-b border-border">
        {tabs.map((t, i) => (
          <button
            key={t.key}
            ref={(el) => {
              refs.current[i] = el;
            }}
            type="button"
            role="tab"
            id={`${id}-${t.key}`}
            aria-selected={value === t.key}
            aria-controls={`${id}-panel`}
            tabIndex={value === t.key ? 0 : -1}
            onClick={() => onChange(t.key)}
            onKeyDown={(e) => onKey(e, i)}
            className={cn(
              "-mb-px border-b-2 px-3.5 py-2.5 text-[13px] transition-colors duration-150",
              value === t.key ? "border-primary font-medium text-foreground" : "border-transparent text-muted-foreground hover:text-foreground",
            )}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div role="tabpanel" id={`${id}-panel`} aria-labelledby={`${id}-${value}`} className="flex flex-col gap-4">
        {children}
      </div>
    </div>
  );
}
