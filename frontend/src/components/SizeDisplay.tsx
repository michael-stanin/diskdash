import { Tooltip } from "@/components/ui/tooltip";
import { cn } from "@/lib/cn";
import { formatAbsolute, formatBytes, formatBytesParts, formatExactBytes, formatRelative } from "@/lib/format";

interface SizeProps {
  bytes: number;
  /** "large" splits value and unit for stat-card style figures. */
  variant?: "inline" | "large";
  className?: string;
}

/** Human-readable size; exact byte count on hover or focus. */
export function SizeDisplay({ bytes, variant = "inline", className }: SizeProps) {
  const exact = formatExactBytes(bytes);
  if (variant === "large") {
    const { value, unit } = formatBytesParts(bytes);
    return (
      <Tooltip content={exact}>
        <span tabIndex={0} aria-label={`${value} ${unit} (${exact})`} className={cn("num", className)}>
          {value} <span className="text-base font-medium text-muted-foreground">{unit}</span>
        </span>
      </Tooltip>
    );
  }
  return (
    <Tooltip content={exact}>
      <span tabIndex={0} className={cn("num", className)}>
        {formatBytes(bytes)}
      </span>
    </Tooltip>
  );
}

/** "3 months ago" with the absolute date on hover. */
export function RelativeTime({ date, className }: { date: string | Date; className?: string }) {
  return (
    <Tooltip content={formatAbsolute(date)}>
      <time tabIndex={0} dateTime={typeof date === "string" ? date : date.toISOString()} className={className}>
        {formatRelative(date)}
      </time>
    </Tooltip>
  );
}
