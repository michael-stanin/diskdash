import type { DrivesResponse } from "@/api/types/DrivesResponse";

/** "2 drives not shown (USB, optical)…" or null when nothing was excluded. */
export function excludedNote(r: DrivesResponse): string | null {
  const e = r.excluded;
  const parts = [
    e.usb > 0 && "USB",
    e.removable > 0 && "removable",
    e.optical > 0 && "optical",
    e.network > 0 && "network",
    e.virtual > 0 && "virtual",
  ].filter(Boolean);
  const total = e.usb + e.removable + e.optical + e.network + e.virtual;
  if (total === 0) return null;
  return `${total} drive${total === 1 ? "" : "s"} not shown (${parts.join(", ")}). Only fixed internal drives are analysed.`;
}
