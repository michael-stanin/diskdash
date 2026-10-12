import type { HTMLAttributes } from "react";
import { cn } from "@/lib/cn";

/** Placeholder block shaped like the content it stands in for. */
export function Skeleton({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div aria-hidden className={cn("animate-shimmer rounded-md bg-muted", className)} {...props} />;
}
