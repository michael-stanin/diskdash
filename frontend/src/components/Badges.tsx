import { cn } from "@/lib/cn";

export type MediaClass = "NVME_SSD" | "SATA_SSD" | "HDD" | "UNKNOWN";
export type Risk = "safe" | "review" | "expert";

const MEDIA: Record<MediaClass, { label: string; className: string; title: string }> = {
  NVME_SSD: { label: "NVMe", className: "bg-media-nvme-bg text-media-nvme-fg", title: "NVMe SSD" },
  SATA_SSD: { label: "SSD", className: "bg-media-ssd-bg text-media-ssd-fg", title: "SATA SSD" },
  HDD: { label: "HDD", className: "bg-media-hdd-bg text-media-hdd-fg", title: "Hard disk drive" },
  UNKNOWN: { label: "Unknown", className: "bg-media-unknown-bg text-media-unknown-fg", title: "Media type unknown" },
};

const RISK: Record<Risk, { label: string; className: string; title: string }> = {
  safe: { label: "Safe", className: "bg-risk-safe-bg text-risk-safe-fg", title: "Safe to remove" },
  review: { label: "Review", className: "bg-risk-review-bg text-risk-review-fg", title: "Check before removing" },
  expert: { label: "Expert", className: "bg-risk-expert-bg text-risk-expert-fg", title: "Only if you know the impact" },
};

const base = "inline-flex items-center rounded-md px-2 py-0.5 text-[11px] font-semibold tracking-wide";

export function MediaBadge({ media, className }: { media: MediaClass; className?: string }) {
  const m = MEDIA[media];
  return (
    <span title={m.title} className={cn(base, m.className, className)}>
      {m.label}
    </span>
  );
}

export function RiskBadge({ risk, className }: { risk: Risk; className?: string }) {
  const r = RISK[risk];
  return (
    <span title={r.title} className={cn(base, r.className, className)}>
      {r.label}
    </span>
  );
}
