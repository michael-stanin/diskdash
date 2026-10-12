import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import type { Drive } from "@/api/types/Drive";
import type { DrivesResponse } from "@/api/types/DrivesResponse";
import { fillState } from "@/components/FillBar";
import { TooltipProvider } from "@/components/ui/tooltip";
import { DriveCard, lowSpaceText } from "./DriveCard";
import { excludedNote } from "./excluded";

const GIB = 1024 ** 3;

function drive(over: Partial<Drive> = {}): Drive {
  const capacity = 931 * GIB;
  const free = 40 * GIB;
  return {
    id: "ea541d5d-0000-0000-0000-100000000000",
    volume_path: "\\\\?\\Volume{ea541d5d-0000-0000-0000-100000000000}\\",
    letters: ["D"],
    mount_paths: [],
    label: "",
    filesystem: "NTFS",
    serial: null,
    cluster_size: 4096,
    capacity,
    free,
    used: capacity - free,
    used_fraction: (capacity - free) / capacity,
    media: "HDD",
    physical_disks: [],
    health: "healthy",
    is_system_volume: false,
    low_space: true,
    low_space_reason: "percent",
    ...over,
  };
}

describe("fill states", () => {
  it.each([
    [0.4, false, "normal"],
    [0.74, false, "normal"],
    [0.75, false, "caution"],
    [0.95, false, "caution"],
    [0.5, true, "warning"],
  ] as const)("used %d low %s → %s", (used, low, expected) => {
    expect(fillState(used, low)).toBe(expected);
  });
});

describe("DriveCard", () => {
  it("shows free space, media and the low-space reason", () => {
    render(
      <MemoryRouter>
        <TooltipProvider>
          <DriveCard drive={drive()} thresholds={{ percent: 10, floorGib: 20 }} />
        </TooltipProvider>
      </MemoryRouter>,
    );
    expect(screen.getByText("40")).toBeInTheDocument();
    expect(screen.getByText("HDD")).toBeInTheDocument();
    expect(screen.getByText("Below 10% free")).toBeInTheDocument();
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "96");
    expect(screen.getByRole("link", { name: "Details for D:" })).toHaveAttribute("href", `/drives/${drive().id}`);
  });

  it("names the reason", () => {
    expect(lowSpaceText(drive({ low_space_reason: "floor" }), 5, 20)).toBe("Below 20 GiB free");
    expect(lowSpaceText(drive({ low_space_reason: "both" }), 10, 20)).toBe("Below 10% and 20 GiB free");
    expect(lowSpaceText(drive({ low_space: false, low_space_reason: null }), 10, 20)).toBeNull();
  });
});

describe("excludedNote", () => {
  const base = { excluded: { usb: 0, removable: 0, optical: 0, network: 0, virtual: 0 } } as DrivesResponse;
  it("is null when nothing is hidden", () => {
    expect(excludedNote(base)).toBeNull();
  });
  it("lists what is hidden", () => {
    expect(excludedNote({ ...base, excluded: { ...base.excluded, usb: 1, optical: 1 } })).toBe(
      "2 drives not shown (USB, optical). Only fixed internal drives are analysed.",
    );
  });
});
