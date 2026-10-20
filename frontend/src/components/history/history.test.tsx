import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router";
import { ApiError } from "@/api/client";
import type { DiffResponse } from "@/api/types/DiffResponse";
import type { Forecast } from "@/api/types/Forecast";
import type { Snapshot } from "@/api/types/Snapshot";
import type { VolumeTrend } from "@/api/types/VolumeTrend";
import { ToastProvider } from "@/components/Toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { HistoryPage } from "@/pages/HistoryPage";
import { DriveCard } from "@/components/drives/DriveCard";
import { forecastText, formatDays, GAP_MS, trendSeries } from "./trend";

const apiMock = vi.fn();
vi.mock("@/api/client", async (orig) => ({ ...(await orig<typeof import("@/api/client")>()), api: (...a: unknown[]) => apiMock(...a) }));
vi.mock("@/app/StatusCluster", () => ({ StatusCluster: () => null }));
// The chart itself needs a canvas; the page only has to mount it.
vi.mock("@/components/history/FreeSpaceChart", () => ({ default: ({ label }: { label: string }) => <div role="img" aria-label={label} /> }));

const GIB = 1024 ** 3;
const DAY = 86_400;

function wrap(ui: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <TooltipProvider>
          <ToastProvider>{ui}</ToastProvider>
        </TooltipProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const snap = (id: number, over: Partial<Snapshot> = {}): Snapshot => ({
  id,
  root: "D:\\",
  volume_id: "v",
  status: "complete",
  method: "walk",
  elevated: false,
  hardlinks_deduplicated: true,
  started_at: `2026-10-${10 + id}T10:00:00Z`,
  finished_at: `2026-10-${10 + id}T10:05:00Z`,
  resumed: false,
  file_count: 10,
  dir_count: 2,
  link_count: 0,
  total_size: 10 * GIB,
  total_alloc: 10 * GIB,
  error_count: 0,
  access_denied_count: 0,
  link_unknown_count: 0,
  job_id: null,
  error: null,
  time_basis: "mtime",
  time_basis_reason: "disabled",
  pinned: false,
  summarised: false,
  summary_depth: null,
  scan_note: null,
  ...over,
});

const filling: Forecast = { state: "filling", days_to_full: 50, days_to_low: 20, bytes_per_day: GIB, r2: 0.99, low_confidence: false, points: 30, span_days: 30 };

const trend = (over: Partial<VolumeTrend> = {}): VolumeTrend => ({
  volume_id: "v",
  label: "Data",
  letters: ["D"],
  capacity: 500 * GIB,
  samples: [
    { ts: 1_700_000_000, free: 100 * GIB, source: "inventory" },
    { ts: 1_700_000_000 + DAY, free: 99 * GIB, source: "scan" },
  ],
  forecast: filling,
  ...over,
});

const diffResponse = (over: Partial<DiffResponse> = {}): DiffResponse => ({
  old: snap(1),
  new: snap(2),
  total_delta: 12 * GIB,
  growing: [{ path: "D:\\Downloads", depth: 1, old_alloc: 10 * GIB, new_alloc: 25 * GIB, delta: 15 * GIB, old_files: 3, new_files: 9 }],
  shrinking: [{ path: "D:\\Games", depth: 1, old_alloc: 30 * GIB, new_alloc: 27 * GIB, delta: -3 * GIB, old_files: 3, new_files: 2 }],
  appeared: [{ path: "D:\\New", depth: 1, old_alloc: 0, new_alloc: 8 * GIB, delta: 8 * GIB, old_files: 0, new_files: 4 }],
  disappeared: [],
  categories: [{ category: "video", old_alloc: 5 * GIB, new_alloc: 17 * GIB, delta: 12 * GIB }],
  new_large_files: [{ path: "D:\\Downloads\\big.iso", alloc: 4 * GIB }],
  new_large_available: true,
  depth_limit: null,
  ...over,
});

function route(map: Record<string, unknown>) {
  apiMock.mockImplementation((path: string) => {
    const hit = Object.keys(map).find((k) => path.startsWith(k));
    if (!hit) return Promise.reject(new ApiError(404, null, `no route ${path}`));
    const v = map[hit];
    return v instanceof Error ? Promise.reject(v) : Promise.resolve(v);
  });
}

beforeEach(() => {
  apiMock.mockReset();
});

describe("forecastText", () => {
  it("is honest when there is not enough history", () => {
    const t = forecastText({ state: "not-enough-history", points: 2, span_days: 1.5 });
    expect(t.headline).toBe("Not enough history yet");
    expect(t.detail).toMatch(/at least 3 samples over 7 days/);
    expect(t.detail).toMatch(/2 over 1\.5 days/);
  });

  it("says when free space is stable", () => {
    expect(forecastText({ state: "not-filling", points: 10, span_days: 20 }).headline).toBe("Free space is stable");
  });

  it("gives days to the low-space limit and to full, and warns when soon", () => {
    const t = forecastText(filling);
    expect(t.headline).toBe("Low in about 20 days");
    expect(t.detail).toMatch(/full in about 50 days/);
    expect(t.tone).toBe("warn");
  });

  it("admits a rough estimate", () => {
    expect(forecastText({ ...filling, low_confidence: true, r2: 0.2 }).detail).toMatch(/rough estimate/);
  });

  it("reports a drive that is already below the limit", () => {
    expect(forecastText({ ...filling, days_to_low: 0 }).headline).toBe("Already below the low-space limit");
  });

  it.each([
    [0.4, "less than a day"],
    [1, "1 day"],
    [45, "45 days"],
    [200, "7 months"],
    [1000, "2.7 years"],
  ])("formats %d days", (d, text) => expect(formatDays(d)).toBe(text));
});

describe("trendSeries", () => {
  it("breaks the line at a gap instead of dropping to zero", () => {
    const t = trend({
      samples: [
        { ts: 1_700_000_000, free: 100 * GIB, source: "inventory" },
        { ts: 1_700_000_000 + 600, free: 99 * GIB, source: "inventory" },
        { ts: 1_700_000_000 + 10 * DAY, free: 90 * GIB, source: "inventory" },
      ],
    });
    const { line } = trendSeries(t);
    expect(line).toHaveLength(4);
    expect(line[2]![1]).toBeNull();
    expect(line[3]![0] - line[1]![0]).toBeGreaterThan(GAP_MS);
    expect(line.filter((p) => p[1] === 0)).toHaveLength(0);
  });

  it("marks scans and extends a dashed forecast from the last sample", () => {
    const { scans, forecast } = trendSeries(trend());
    expect(scans).toEqual([[(1_700_000_000 + DAY) * 1000, 99 * GIB]]);
    expect(forecast).toHaveLength(2);
    expect(forecast[0]![1]).toBe(99 * GIB);
    expect(forecast[1]![1]).toBe(49 * GIB);
  });

  it("draws no forecast without one", () => {
    expect(trendSeries(trend({ forecast: { state: "not-filling", points: 5, span_days: 9 } })).forecast).toEqual([]);
  });
});

describe("DriveCard forecast", () => {
  it("shows the outlook on the card", () => {
    const d = {
      id: "v", volume_path: "", letters: ["D"], mount_paths: [], label: "", filesystem: "NTFS", serial: null, cluster_size: 4096,
      capacity: 500 * GIB, free: 100 * GIB, used: 400 * GIB, used_fraction: 0.8, media: "HDD", physical_disks: [], health: "healthy",
      is_system_volume: false, low_space: false, low_space_reason: null,
    } as never;
    render(
      <MemoryRouter>
        <TooltipProvider>
          <DriveCard drive={d} thresholds={{ percent: 10, floorGib: 20 }} forecast={filling} />
        </TooltipProvider>
      </MemoryRouter>,
    );
    expect(screen.getByText(/Forecast: Low in about 20 days/)).toBeInTheDocument();
  });
});

describe("HistoryPage", () => {
  it("explains that two scans are needed and offers a rescan", async () => {
    route({ "/history/volumes": { volumes: [trend()] }, "/snapshots": [snap(1)], "/scans": { job_id: 1, snapshots: [{ id: 2, root: "D:\\" }], merged: [] } });
    wrap(<HistoryPage />);
    expect(await screen.findByText("Need two scans of this folder to compare")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Rescan now/ }));
    await waitFor(() => expect(apiMock).toHaveBeenCalledWith("/scans", expect.objectContaining({ method: "POST", body: { roots: ["D:\\"] } })));
  });

  it("compares the latest scan with the previous one by default and lists what changed", async () => {
    route({
      "/history/volumes": { volumes: [trend()] },
      "/snapshots": [snap(2), snap(1)],
      "/history/diff": diffResponse(),
    });
    wrap(<HistoryPage />);
    expect(await screen.findByRole("list", { name: "Grew the most" })).toBeInTheDocument();
    expect(apiMock).toHaveBeenCalledWith(expect.stringContaining("/history/diff?old=1&new=2"), expect.anything());
    const grew = screen.getByRole("list", { name: "Grew the most" });
    expect(within(grew).getByText("D:\\Downloads")).toBeInTheDocument();
    expect(within(grew).getByText("+15 GiB")).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "Shrank the most" })).getByText("−3 GiB")).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "New folders" })).getByText("D:\\New")).toBeInTheDocument();
    expect(screen.getByText("No folders were removed.")).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "New large files" })).getByText("D:\\Downloads\\big.iso")).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "Change by file type" })).getByText("Video")).toBeInTheDocument();
  });

  it("explains the limits of a scan that was reduced to folder totals", async () => {
    route({
      "/history/volumes": { volumes: [] },
      "/snapshots": [snap(3), snap(1, { summarised: true, summary_depth: 4 })],
      "/history/diff": diffResponse({ old: snap(1, { summarised: true, summary_depth: 4 }), new: snap(3), new_large_available: false, new_large_files: [], depth_limit: 4 }),
    });
    wrap(<HistoryPage />);
    expect(await screen.findByText(/reduced to folder totals down to level 4/)).toBeInTheDocument();
    expect(screen.getByText(/Not available: the older scan was reduced to folder totals/)).toBeInTheDocument();
  });

  it("shows the forecast, the caveat and a chart for each drive", async () => {
    route({ "/history/volumes": { volumes: [trend()] }, "/snapshots": [] });
    wrap(<HistoryPage />);
    const card = await screen.findByLabelText("Free space of D: Data");
    expect(within(card).getByText("Low in about 20 days")).toBeInTheDocument();
    expect(within(card).getByText(/only an estimate/)).toBeInTheDocument();
    expect(await within(card).findByRole("img")).toBeInTheDocument();
  });

  it("says plainly when there is not enough history", async () => {
    route({
      "/history/volumes": { volumes: [trend({ samples: [{ ts: 1_700_000_000, free: GIB, source: "inventory" }], forecast: { state: "not-enough-history", points: 1, span_days: 0 } })] },
      "/snapshots": [],
    });
    wrap(<HistoryPage />);
    const card = await screen.findByLabelText("Free space of D: Data");
    expect(within(card).getByText("Not enough history yet")).toBeInTheDocument();
    expect(within(card).queryByRole("img")).toBeNull();
  });

  it("shows an error with retry when the history cannot be loaded", async () => {
    route({ "/history/volumes": new ApiError(500, null, "boom"), "/snapshots": [] });
    wrap(<HistoryPage />);
    expect(await screen.findByText("Could not load free-space history")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Retry/ })).toBeInTheDocument();
  });
});
