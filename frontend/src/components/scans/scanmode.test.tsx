import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router";
import { ApiError } from "@/api/client";
import type { Drive } from "@/api/types/Drive";
import type { HelperStatus } from "@/api/types/HelperStatus";
import type { ScanModes } from "@/api/types/ScanModes";
import type { Snapshot } from "@/api/types/Snapshot";
import { ToastProvider } from "@/components/Toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { SnapshotSummary } from "@/components/usage/SnapshotSummary";
import { ScanDialogProvider, useScanDialog } from "./ScanDialog";
import { ModeBadge, QuickRescanButton } from "./ScanMode";

const apiMock = vi.fn();
vi.mock("@/api/client", async (orig) => ({ ...(await orig<typeof import("@/api/client")>()), api: (...a: unknown[]) => apiMock(...a) }));

const GIB = 1024 ** 3;

function wrap(ui: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <TooltipProvider>
          <ToastProvider>
            <ScanDialogProvider>{ui}</ScanDialogProvider>
          </ToastProvider>
        </TooltipProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const drive = (letter: string, over: Partial<Drive> = {}): Drive =>
  ({
    id: `vol-${letter}`,
    volume_path: `\\\\?\\Volume{${letter}}\\`,
    letters: [letter],
    mount_paths: [],
    label: "",
    filesystem: "NTFS",
    serial: null,
    cluster_size: 4096,
    capacity: 500 * GIB,
    free: 100 * GIB,
    used: 400 * GIB,
    used_fraction: 0.8,
    media: "NVME_SSD",
    physical_disks: [],
    health: "healthy",
    is_system_volume: false,
    low_space: false,
    low_space_reason: null,
    ...over,
  }) as Drive;

const status = (state: HelperStatus["state"], message: string | null = null): HelperStatus => ({ state, message });

const modes = (helper: HelperStatus, fast: boolean): ScanModes => ({
  helper,
  drives: [
    { root: "C:\\", volume_id: "vol-C", mode: fast ? "mft" : "walk", reason: fast ? null : "fast scan requires administrator rights" },
    { root: "E:\\", volume_id: "vol-E", mode: "walk", reason: "MFT scan supports NTFS only" },
  ],
});

function route(map: Record<string, unknown>) {
  apiMock.mockImplementation((path: string) => {
    const hit = Object.keys(map).find((k) => path.startsWith(k));
    if (!hit) return Promise.reject(new ApiError(404, null, `no route ${path}`));
    const v = map[hit];
    return v instanceof Error ? Promise.reject(v) : Promise.resolve(typeof v === "function" ? (v as () => unknown)() : v);
  });
}

function Opener() {
  const d = useScanDialog();
  return <button onClick={() => d.open([])}>open scan dialog</button>;
}

const drives = { drives: [drive("C"), drive("E", { filesystem: "exFAT" })], excluded: {}, trim: {}, sources: { elevated: false }, fallback: false, refreshed_at: null, stale: false, error: null };

beforeEach(() => {
  apiMock.mockReset();
});

describe("ModeBadge", () => {
  it("names each way of scanning", () => {
    render(
      <>
        <ModeBadge method="mft" />
        <ModeBadge method="mft-quick" />
        <ModeBadge method="walk" />
      </>,
    );
    expect(screen.getByText("fast scan (MFT)")).toBeInTheDocument();
    expect(screen.getByText(/quick rescan/)).toBeInTheDocument();
    expect(screen.getByText("directory walk")).toBeInTheDocument();
  });
});

describe("scan dialog", () => {
  async function openDialog() {
    wrap(<Opener />);
    await userEvent.click(screen.getByText("open scan dialog"));
  }

  it("shows, for every drive, whether it will be scanned fast or walked, and why", async () => {
    route({ "/drives": drives, "/scans/modes": modes(status("not-running"), false) });
    await openDialog();
    const c = (await screen.findByLabelText(/Scan C:/)).closest("label")!;
    expect(await within(c).findByText("directory walk")).toBeInTheDocument();
    expect(within(c).getByText("fast scan requires administrator rights")).toBeInTheDocument();
    const e = screen.getByLabelText(/Scan E:/).closest("label")!;
    expect(within(e).getByText("MFT scan supports NTFS only")).toBeInTheDocument();
    expect(screen.getByText(/single folder is always scanned with the directory walk/)).toBeInTheDocument();
  });

  it("offers to enable elevated features and shows progress while the Windows prompt is open", async () => {
    let answer: (s: HelperStatus) => void = () => {};
    route({
      "/drives": drives,
      "/scans/modes": modes(status("not-running"), false),
      "/helper/enable": () => new Promise<HelperStatus>((r) => (answer = r)),
    });
    await openDialog();
    await userEvent.click(await screen.findByRole("button", { name: "Enable elevated features" }));
    const waiting = await screen.findByRole("button", { name: /Waiting for the Windows prompt/ });
    expect(waiting).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent(/Approve the Windows administrator prompt/);
    expect(apiMock).toHaveBeenCalledWith("/helper/enable", expect.objectContaining({ method: "POST" }));
    route({ "/drives": drives, "/scans/modes": modes(status("running"), true) });
    answer(status("running"));
    expect(await screen.findByText("Elevated helper running")).toBeInTheDocument();
    expect(await screen.findByText("fast scan (MFT)")).toBeInTheDocument();
  });

  it("shows an error banner and stays in reduced mode when the prompt is declined or the helper fails", async () => {
    route({
      "/drives": drives,
      "/scans/modes": modes(status("not-running"), false),
      "/helper/enable": new ApiError(409, { error: "conflict", message: "The administrator prompt was declined, so fast scans stay off.", fields: {} }, "declined"),
    });
    await openDialog();
    await userEvent.click(await screen.findByRole("button", { name: "Enable elevated features" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(/declined/);
    expect(alert).toHaveTextContent(/directory walk/);
    expect(screen.getByRole("button", { name: "Enable elevated features" })).toBeEnabled();
    expect(screen.getAllByText("directory walk").length).toBeGreaterThan(0);
  });

  it("explains a helper that is missing or stopped", async () => {
    route({ "/drives": drives, "/scans/modes": modes(status("unavailable"), false) });
    await openDialog();
    expect(await screen.findByText("Fast scan is not installed")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Enable elevated features" })).toBeNull();
  });

  it("reports a helper that stopped on its own", async () => {
    route({ "/drives": drives, "/scans/modes": modes(status("failed", "the elevated helper stopped (exit code 1)"), false) });
    await openDialog();
    expect(await screen.findByRole("alert")).toHaveTextContent(/stopped \(exit code 1\)/);
  });
});

const snap = (over: Partial<Snapshot> = {}): Snapshot =>
  ({
    id: 1,
    root: "C:\\",
    volume_id: "vol-C",
    status: "complete",
    method: "mft",
    elevated: false,
    hardlinks_deduplicated: true,
    started_at: "2026-10-10T10:00:00Z",
    finished_at: "2026-10-10T10:00:08Z",
    resumed: false,
    file_count: 100,
    dir_count: 10,
    link_count: 0,
    total_size: GIB,
    total_alloc: GIB,
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
  }) as Snapshot;

describe("snapshot summary and quick rescan", () => {
  it("shows how the scan was made and why a walk was used", () => {
    const { rerender } = render(
      <TooltipProvider>
        <SnapshotSummary snapshot={snap()} />
      </TooltipProvider>,
    );
    expect(screen.getByText("fast scan (MFT)")).toBeInTheDocument();
    rerender(
      <TooltipProvider>
        <SnapshotSummary snapshot={snap({ method: "walk", scan_note: "fast scan requires administrator rights" })} />
      </TooltipProvider>,
    );
    expect(screen.getByText("directory walk")).toBeInTheDocument();
    expect(screen.getByText(/fast scan requires administrator rights/)).toBeInTheDocument();
  });

  it("quick rescan starts a rescan, shows progress and reports a refusal", async () => {
    let finish: (v: unknown) => void = () => {};
    route({ "/scans/quick": () => new Promise((r) => (finish = r)) });
    wrap(<QuickRescanButton root={"C:\\"} />);
    await userEvent.click(screen.getByRole("button", { name: "Quick rescan" }));
    expect(await screen.findByRole("button", { name: /Checking changes/ })).toBeDisabled();
    expect(apiMock).toHaveBeenCalledWith("/scans/quick", expect.objectContaining({ body: { root: "C:\\" } }));
    finish({ job_id: 5, snapshots: [{ id: 9, root: "C:\\", method: "mft-quick", note: null }], merged: [] });
    await waitFor(() => expect(screen.getByRole("button", { name: "Quick rescan" })).toBeEnabled());
  });

  it("shows the reason when the quick rescan is refused", async () => {
    route({ "/scans/quick": new ApiError(409, { error: "conflict", message: "Quick rescan is not available: fast scan requires administrator rights", fields: {} }, "x") });
    wrap(<QuickRescanButton root={"C:\\"} />);
    await userEvent.click(screen.getByRole("button", { name: "Quick rescan" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/not available: fast scan requires administrator rights/);
  });
});
