import type { ReactNode } from "react";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";

interface StatCardProps {
  label: string;
  value: ReactNode;
  /** Secondary line: a delta, breakdown or note. */
  detail?: ReactNode;
  icon?: ReactNode;
  tone?: "default" | "accent";
  className?: string;
}

export function StatCard({ label, value, detail, icon, tone = "default", className }: StatCardProps) {
  return (
    <Card
      className={cn(
        "flex flex-col gap-2 p-[18px]",
        tone === "accent" && "border-primary/30 bg-primary-soft/40",
        className,
      )}
    >
      <div className="flex items-center gap-2 text-[13px] text-muted-foreground">
        {icon && <span className="text-subtle-foreground [&_svg]:size-4">{icon}</span>}
        {label}
      </div>
      <div className="num text-display font-semibold">{value}</div>
      {detail && <div className="text-xs text-muted-foreground">{detail}</div>}
    </Card>
  );
}

export function StatCardSkeleton() {
  return (
    <Card className="flex flex-col gap-3 p-[18px]">
      <Skeleton className="h-4 w-24" />
      <Skeleton className="h-8 w-28" />
      <Skeleton className="h-3 w-32" />
    </Card>
  );
}
