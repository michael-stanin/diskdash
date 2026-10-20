import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router";
import { ApiError } from "@/api/client";
import type { AlertView } from "@/api/types/AlertView";
import type { AlertsResponse } from "@/api/types/AlertsResponse";
import type { Drive } from "@/api/types/Drive";
import type { SectionInfo } from "@/api/types/SectionInfo";
import { ExportButton } from "@/components/reports/ExportDialog";
import { ScanDialogProvider } from "@/components/scans/ScanDialog";
import { ToastProvider } from "@/components/Toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AlertsPage } from "@/pages/AlertsPage";
import { AlertBanners } from "./AlertBanners";
import { AlertBell } from "./AlertBell";

const apiMock = vi.fn();
const downloadMock = vi.fn();
vi.mock("@/api/client", async (orig) => ({
  ...(await orig<typeof import("@/api/client")>()),
  api: (...a: unknown[]) => apiMock(...a),
  apiDownload: (...a: unknown[]) => downloadMock(...a),
}));
vi.mock("@/app/StatusCluster", () => ({ StatusCluster: () => null }));

function wrap(ui: ReactNode, route = "/") {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[route]}>
        <TooltipProvider>
          <ToastProvider>
            <ScanDialogProvider>{ui}</ScanDialogProvider>
          </ToastProvider>
        </TooltipProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const alert = (over: Partial<AlertView> = {}): AlertView => ({
  id: 1,
  kind: "low_space",
  severity: "warning",
  drive: "D:",
  volume_id: "vol-D",
  path: null,
  title: "D: is low on space",
  message: "40.0 GiB free of 931.5 GiB (4.3%). The limit is 10% or 20 GiB.",
  value: 40,
  threshold: 90,
  raised_at: "2026-10-20T10:00:00Z",
  resolved_at: null,
  dismissed: false,
  history: null,
  ...over,
});

const response = (alerts: AlertView[], over: Partial<AlertsResponse> = {}): AlertsResponse => ({
  alerts,
  active: alerts.filter((a) => !a.resolved_at).length,
  banners: alerts.filter((a) => !a.resolved_at && !a.dismissed).length,
  thresholds: { low_percent: 10, low_floor_gib: 20, growth_gib_per_day: 5, cooldown_hours: 24 },
  running_since: "2026-10-20T09:00:00Z",
  ...over,
});

function route(map: Record<string, unknown>) {
  apiMock.mockImplementation((path: string) => {
    const hit = Object.keys(map).find((k) => path.startsWith(k));
    if (!hit) return Promise.reject(new ApiError(404, null, `no route ${path}`));
    const v = map[hit];
    return v instanceof Error ? Promise.reject(v) : Promise.resolve(typeof v === "function" ? (v as (p: string) => unknown)(path) : v);
  });
}

beforeEach(() => {
  apiMock.mockReset();
  downloadMock.mockReset();
});

describe("AlertBell", () => {
  it("shows the number of active alerts", async () => {
    route({ "/alerts": response([alert(), alert({ id: 2, kind: "growth", title: "x" })]) });
    wrap(<AlertBell />);
    expect(await screen.findByLabelText("Alerts: 2 active")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
  });

  it("shows no badge when there are none", async () => {
    route({ "/alerts": response([]) });
    wrap(<AlertBell />);
    expect(await screen.findByLabelText("Alerts: none")).toBeInTheDocument();
  });

  it("shows an unknown state instead of a false zero when alerts cannot be loaded", async () => {
    route({ "/alerts": new ApiError(500, null, "boom") });
    wrap(<AlertBell />);
    expect(await screen.findByLabelText(/state unknown/)).toBeInTheDocument();
    expect(screen.getByText("?")).toBeInTheDocument();
    expect(screen.queryByText("0")).toBeNull();
  });
});

describe("AlertBanners", () => {
  it("shows a banner for a low-space alert and dismisses it", async () => {
    let dismissed = false;
    route({
      "/alerts/1/dismiss": () => {
        dismissed = true;
        return alert({ dismissed: true });
      },
      "/alerts": () => response([alert({ dismissed })]),
    });
    wrap(<AlertBanners />);
    expect(await screen.findByText("D: is low on space")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "View suggestions" })).toHaveAttribute("href", "/recommendations");
    await userEvent.click(screen.getByRole("button", { name: /Dismiss: D: is low on space/ }));
    await waitFor(() => expect(screen.queryByText("D: is low on space")).toBeNull());
    expect(apiMock).toHaveBeenCalledWith("/alerts/1/dismiss", expect.objectContaining({ method: "POST" }));
  });

  it("only shows the alerts of this drive on a drive view, and never growth alerts", async () => {
    route({ "/alerts": response([alert(), alert({ id: 2, volume_id: "vol-C", title: "C: is low on space" }), alert({ id: 3, kind: "growth", title: "growth" })]) });
    wrap(<AlertBanners volumeId="vol-C" />);
    expect(await screen.findByText("C: is low on space")).toBeInTheDocument();
    expect(screen.queryByText("D: is low on space")).toBeNull();
    expect(screen.queryByText("growth")).toBeNull();
  });

  it("shows nothing for dismissed alerts or when the request fails", async () => {
    route({ "/alerts": response([alert({ dismissed: true })]) });
    const { container } = wrap(<AlertBanners />);
    await waitFor(() => expect(apiMock).toHaveBeenCalled());
    expect(container.querySelector('[role="alert"]')).toBeNull();
  });
});

describe("AlertsPage", () => {
  it("lists alerts with links to the drive and to the history comparison", async () => {
    route({
      "/alerts": response([
        alert(),
        alert({ id: 2, kind: "growth", volume_id: "vol-D", path: "D:\\VMs", title: "D:\\VMs is growing fast", message: "D:\\VMs grew by 30.0 GiB in 2.0 days.", history: { old: 3, new: 4 } }),
      ]),
    });
    wrap(<AlertsPage />);
    const list = await screen.findByRole("list", { name: "Alerts" });
    expect(within(list).getByText("D:\\VMs is growing fast")).toBeInTheDocument();
    expect(within(list).getByRole("link", { name: /Compare the scans/ })).toHaveAttribute("href", "/history?old=3&new=4");
    expect(within(list).getAllByRole("link", { name: "Open drive" })[0]).toHaveAttribute("href", "/drives/vol-D");
    expect(screen.getByText(/only checked while diskdash is running/)).toBeInTheDocument();
    expect(screen.getByText(/never sends Windows notifications/)).toBeInTheDocument();
  });

  it("says all drives look healthy, with the limits in use and a link to change them", async () => {
    route({ "/alerts": response([]) });
    wrap(<AlertsPage />);
    expect(await screen.findByText("All drives look healthy")).toBeInTheDocument();
    expect(screen.getByText(/less than 10% or 20 GiB free/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Change these limits" })).toHaveAttribute("href", "/settings");
  });

  it("shows resolved alerts under their own filter", async () => {
    route({ "/alerts": (p: string) => (p.includes("resolved") ? response([alert({ resolved_at: "2026-10-21T10:00:00Z" })]) : response([])) });
    wrap(<AlertsPage />);
    await userEvent.click(await screen.findByRole("button", { name: "Resolved" }));
    expect(await screen.findByText("Resolved", { selector: "span" })).toBeInTheDocument();
    expect(apiMock).toHaveBeenCalledWith(expect.stringContaining("status=resolved"), expect.anything());
  });

  it("shows an error with retry when alerts cannot be loaded", async () => {
    route({ "/alerts": new ApiError(500, null, "boom") });
    wrap(<AlertsPage />);
    expect(await screen.findByText("Could not load alerts")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Retry/ })).toBeInTheDocument();
  });
});

const sec = (id: SectionInfo["id"], title: string, available: boolean, reason: string | null = null): SectionInfo => ({ id, title, available, reason });
const drive = { id: "vol-C", letters: ["C"], label: "Windows" } as unknown as Drive;
const drivesResponse = { drives: [drive], excluded: {}, trim: {}, sources: { elevated: false }, fallback: false, refreshed_at: null, stale: false, error: null };

describe("Export dialog", () => {
  async function open() {
    wrap(<ExportButton />);
    await userEvent.click(screen.getByRole("button", { name: /Export report/ }));
  }

  it("marks sections without data as not available, with the reason", async () => {
    route({
      "/drives": drivesResponse,
      "/reports/sections": [sec("drives", "Drive summary", true), sec("top", "Largest folders and files", true), sec("duplicates", "Duplicate files", false, "No duplicate analysis has completed yet.")],
    });
    await open();
    expect(await screen.findByLabelText("Drive summary")).toBeChecked();
    const dup = screen.getByLabelText("Duplicate files");
    expect(dup).toBeDisabled();
    expect(dup).not.toBeChecked();
    expect(screen.getByText(/Not available: No duplicate analysis has completed yet/)).toBeInTheDocument();
  });

  it("explains that only the summary exists before any scan and offers to scan", async () => {
    route({
      "/drives": drivesResponse,
      "/reports/sections": [sec("drives", "Drive summary", true), sec("top", "Largest folders and files", false, "No drive has been scanned yet.")],
    });
    await open();
    expect(await screen.findByText(/only the drive summary is available/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Scan a drive" })).toBeInTheDocument();
  });

  it("downloads the selected sections in the chosen format", async () => {
    route({ "/drives": drivesResponse, "/reports/sections": [sec("drives", "Drive summary", true), sec("top", "Largest folders and files", true)] });
    downloadMock.mockResolvedValue({ blob: new Blob(["{}"]), fileName: "diskdash-report-1.json" });
    URL.createObjectURL = vi.fn(() => "blob:x");
    URL.revokeObjectURL = vi.fn();
    await open();
    await userEvent.click(await screen.findByLabelText("Largest folders and files"));
    await userEvent.click(screen.getByLabelText(/^JSON/));
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(downloadMock).toHaveBeenCalled());
    expect(downloadMock).toHaveBeenCalledWith("/reports", { format: "json", sections: ["drives"], drive: null, top_n: null });
  });

  it("shows progress while the report is created and an error with retry when it fails", async () => {
    route({ "/drives": drivesResponse, "/reports/sections": [sec("drives", "Drive summary", true)] });
    let fail: (e: Error) => void = () => {};
    downloadMock.mockImplementationOnce(() => new Promise((_, reject) => (fail = reject)));
    await open();
    await userEvent.click(await screen.findByRole("button", { name: "Download" }));
    expect(await screen.findByRole("button", { name: /Creating report/ })).toBeDisabled();
    expect(screen.getByText(/Creating the report from stored scan data/)).toBeInTheDocument();
    fail(new ApiError(500, { error: "internal", message: "the report could not be built", fields: {} }, "x"));
    const alertBox = await screen.findByRole("alert");
    expect(alertBox).toHaveTextContent("the report could not be built");
    downloadMock.mockResolvedValue({ blob: new Blob(["x"]), fileName: "r.html" });
    URL.createObjectURL = vi.fn(() => "blob:x");
    URL.revokeObjectURL = vi.fn();
    await userEvent.click(within(alertBox).getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(downloadMock).toHaveBeenCalledTimes(2));
  });

  it("saves to a folder and shows the file that was written, or the reason it could not be", async () => {
    let call = 0;
    route({
      "/drives": drivesResponse,
      "/reports/sections": [sec("drives", "Drive summary", true)],
      "/reports/save": () => {
        call += 1;
        if (call === 1) throw new ApiError(422, { error: "validation", message: "validation failed", fields: { folder: "C:\\nowhere is not a folder that exists" } }, "x");
        return { path: "D:\\Reports\\diskdash-report-1.html", file_name: "diskdash-report-1.html", bytes: 1234 };
      },
    });
    await open();
    await userEvent.click(await screen.findByText("Save to a folder instead"));
    await userEvent.type(screen.getByLabelText(/A new file is created there/), "C:\\nowhere");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("is not a folder that exists");
    await userEvent.clear(screen.getByLabelText(/A new file is created there/));
    await userEvent.type(screen.getByLabelText(/A new file is created there/), "D:\\Reports");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/Saved diskdash-report-1.html to D:\\Reports\\diskdash-report-1.html/)).toBeInTheDocument();
  });

  it("shows an error when the available sections cannot be determined", async () => {
    route({ "/drives": drivesResponse, "/reports/sections": new ApiError(500, null, "boom") });
    await open();
    expect(await screen.findByText("Could not check what is available")).toBeInTheDocument();
  });
});
