import { useState, type ReactNode } from "react";
import { AlertTriangle, RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/cn";

interface EmptyStateProps {
  icon: ReactNode;
  title: string;
  description?: ReactNode;
  action?: ReactNode;
  className?: string;
}

/** Designed empty state: what is missing and the next step. */
export function EmptyState({ icon, title, description, action, className }: EmptyStateProps) {
  return (
    <div className={cn("flex flex-col items-center justify-center gap-3 px-6 py-14 text-center", className)}>
      <div className="flex size-12 items-center justify-center rounded-xl bg-muted text-muted-foreground [&_svg]:size-6">
        {icon}
      </div>
      <div className="flex max-w-sm flex-col gap-1">
        <h3 className="m-0 text-[15px] font-semibold">{title}</h3>
        {description && <p className="m-0 text-[13px] leading-relaxed text-muted-foreground">{description}</p>}
      </div>
      {action && <div className="mt-1">{action}</div>}
    </div>
  );
}

interface ErrorStateProps {
  title?: string;
  error: unknown;
  onRetry?: () => void;
  className?: string;
}

/** Request failed: message, optional details and a retry action. */
export function ErrorState({ title = "Something went wrong", error, onRetry, className }: ErrorStateProps) {
  const [open, setOpen] = useState(false);
  const message = error instanceof Error ? error.message : String(error);
  return (
    <div role="alert" className={cn("flex flex-col items-center gap-3 px-6 py-12 text-center", className)}>
      <div className="flex size-12 items-center justify-center rounded-xl bg-danger-soft text-danger [&_svg]:size-6">
        <AlertTriangle />
      </div>
      <h3 className="m-0 text-[15px] font-semibold">{title}</h3>
      <div className="flex gap-2">
        {onRetry && (
          <Button variant="secondary" onClick={onRetry}>
            <RotateCw /> Retry
          </Button>
        )}
        <Button variant="ghost" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
          {open ? "Hide details" : "Details"}
        </Button>
      </div>
      {open && (
        <pre className="m-0 max-w-lg overflow-auto whitespace-pre-wrap rounded-lg bg-muted px-3 py-2 text-left font-mono text-xs text-muted-foreground">
          {message}
        </pre>
      )}
    </div>
  );
}
