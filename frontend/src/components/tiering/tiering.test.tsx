import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router";
import type { TierDrive } from "@/api/types/TierDrive";
import type { TierSuggestion } from "@/api/types/TierSuggestion";
import type { TieringResponse } from "@/api/types/TieringResponse";
import { ToastProvider } from "@/components/Toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { TieringPage } from "@/pages/TieringPage";

const apiMock = vi.fn();
vi.mock("@/api/client", async (orig) => ({ ...(await orig<typeof import("@/api/client")>()), api: (...a: unknown[]) => apiMock(...a) }));
vi.mock("@/app/StatusCluster", () => ({ StatusCluster: () => null }));

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

const GIB = 1024 ** 3;
const drive = (over: Partial<TierDrive> = {}): TierDrive => ({
  id: "c",
  letter: "C",
  media: "NVME_SSD",
  capacity: 500 * GIB,
  free: 100 * GIB,
  projected_free: 100 * GIB,
  low_after: false,
  scanned: true,
  ...over,
});

const suggestion = (over: Partial<TierSuggestion> = {}): TierSuggestion => ({
  key: "c:\\videos\\2019",
  direction: "cold-to-hdd",
  kind: "data",
  path: "C:\\Videos\\2019",
  name: "2019",
  is_dir: true,
  snapshot_id: 1,
  entry_id: 5,
  allocated: 80 * GIB,
  source_drive: "c",
  reason: "not modified for 3.0 years",
  app: null,
  target: { drive_id: "d", letter: "D" },
  no_target_reason: null,
  destination: "D:\\Videos\\2019",
  planned: false,
  instructions: ["Copy it to the other drive. Run this yourself in a terminal (diskdash never runs it):", 'robocopy "C:\\Videos\\2019" "D:\\Videos\\2019" /E /COPY:DAT /DCOPY:T'],
  ...over,
});

const response = (over: Partial<TieringResponse> = {}): TieringResponse => ({
  suggestions: [suggestion()],
  drives: [drive(), drive({ id: "d", letter: "D", media: "HDD", capacity: 1000 * GIB, free: 600 * GIB, projected_free: 600 * GIB })],
  notes: [],
  thresholds: { cold_months: 12, min_gib: 1, hot_days: 30, low_percent: 10, low_floor_gib: 20 },
  as_of: "2026-10-15T10:00:00Z",
  manifest_errors: 0,
  ...over,
});

beforeEach(() => {
  apiMock.mockReset();
});

describe("TieringPage", () => {
  it("shows a skeleton while loading", () => {
    apiMock.mockReturnValue(new Promise(() => {}));
    wrap(<TieringPage />);
    expect(screen.getByLabelText("Loading tiering")).toBeInTheDocument();
  });

  it("shows an error with retry while the drive bars still render from the inventory", async () => {
    apiMock.mockImplementation((p: string) => {
      if (p === "/tiering") return Promise.reject(new Error("boom"));
      if (p === "/drives") return Promise.resolve({ drives: [{ id: "c", letters: ["C"], media: "NVME_SSD", capacity: 500 * GIB, free: 50 * GIB, low_space: true }] });
      return Promise.resolve(null);
    });
    wrap(<TieringPage />);
    expect(await screen.findByText("Cannot work out tiering suggestions")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Retry/ })).toBeInTheDocument();
    expect(await screen.findByLabelText("Drive C")).toBeInTheDocument();
  });

  it("explains why there is nothing to suggest, with the thresholds and a link to settings", async () => {
    apiMock.mockResolvedValue(response({ suggestions: [], notes: [{ code: "no_hdd_tier", message: "No internal hard disk (HDD) was detected." }] }));
    wrap(<TieringPage />);
    expect(await screen.findByText("No tiering suggestions")).toBeInTheDocument();
    expect(screen.getByText(/No internal hard disk/)).toBeInTheDocument();
    expect(screen.getByText(/not modified for 12 months and at least 1 GiB/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Adjust the thresholds/ })).toHaveAttribute("href", "/settings");
  });

  it("lists a cold suggestion with its target, and copyable manual instructions that are never run", async () => {
    const user = userEvent.setup();
    apiMock.mockResolvedValue(response());
    wrap(<TieringPage />);
    const row = await screen.findByRole("listitem", { name: "2019" });
    expect(within(row).getByText("not modified for 3.0 years")).toBeInTheDocument();
    expect(within(row).getByText(/Could go to D:/)).toBeInTheDocument();
    expect(within(row).getByText("80 GiB")).toBeInTheDocument();
    await user.click(within(row).getByRole("button", { name: /How to move/ }));
    expect(screen.getByRole("button", { name: /Copy command robocopy/ })).toBeInTheDocument();
    expect(screen.getByText(/only shows these steps/)).toBeInTheDocument();
    for (const bad of [/^move now/i, /^run/i, /execute/i, /^delete/i]) expect(within(row).queryByRole("button", { name: bad })).not.toBeInTheDocument();
  });

  it("planning a move updates the projected free space", async () => {
    const user = userEvent.setup();
    apiMock.mockImplementation((p: string, o?: { method?: string; body?: { planned: boolean } }) => {
      if (p === "/tiering/planned") {
        return Promise.resolve(
          response({
            suggestions: [suggestion({ planned: o?.body?.planned ?? false })],
            drives: [drive({ projected_free: 180 * GIB }), drive({ id: "d", letter: "D", media: "HDD", capacity: 1000 * GIB, free: 600 * GIB, projected_free: 520 * GIB })],
          }),
        );
      }
      return Promise.resolve(response());
    });
    wrap(<TieringPage />);
    const row = await screen.findByRole("listitem", { name: "2019" });
    await user.click(within(row).getByRole("checkbox"));
    await waitFor(() => expect(apiMock).toHaveBeenCalledWith("/tiering/planned", expect.objectContaining({ method: "POST", body: { path: "C:\\Videos\\2019", planned: true } })));
    const c = await screen.findByLabelText("Drive C");
    await waitFor(() => expect(within(c).getByText("180 GiB")).toBeInTheDocument());
    expect(within(c).getByText(/\(\+80 GiB\)/)).toBeInTheDocument();
    expect(within(screen.getByLabelText("Drive D")).getByText(/\(-80 GiB\)/)).toBeInTheDocument();
  });

  it("shows no_target_with_space and disables planning", async () => {
    apiMock.mockResolvedValue(response({ suggestions: [suggestion({ target: null, no_target_reason: "no_target_with_space", destination: null })] }));
    wrap(<TieringPage />);
    const row = await screen.findByRole("listitem", { name: "2019" });
    expect(within(row).getByText(/No drive has room/)).toBeInTheDocument();
    expect(within(row).getByText(/no_target_with_space/)).toBeInTheDocument();
    expect(within(row).getByRole("checkbox")).toBeDisabled();
    expect(within(row).queryByRole("button", { name: /Add to plan/ })).not.toBeInTheDocument();
  });

  it("labels app libraries and installs and warns about a drive that would run low", async () => {
    apiMock.mockResolvedValue(
      response({
        suggestions: [
          suggestion({ direction: "hot-to-ssd", kind: "app-library", app: "Big Game", name: "BigGame", path: "D:\\Steam\\BigGame", key: "d:\\steam\\biggame", reason: "Steam game last played 2 days ago (app library, Steam)" }),
          suggestion({ kind: "app-install", app: "Big App", name: "BigApp", path: "C:\\Apps\\BigApp", key: "c:\\apps\\bigapp" }),
        ],
        drives: [drive({ projected_free: 10 * GIB, low_after: true })],
      }),
    );
    wrap(<TieringPage />);
    expect(await screen.findByText("App library")).toBeInTheDocument();
    expect(screen.getByText("App install")).toBeInTheDocument();
    expect(screen.getByText("Would be low on space")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Busy data on HDDs" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Cold data on SSDs" })).toBeInTheDocument();
  });

  it("mentions drives that are not scanned and unreadable manifests", async () => {
    apiMock.mockResolvedValue(response({ notes: [{ code: "unscanned", message: "Not scanned yet: D:." }], manifest_errors: 2 }));
    wrap(<TieringPage />);
    expect(await screen.findByRole("note")).toHaveTextContent("Not scanned yet: D:.");
    expect(screen.getByText(/2 game launcher files could not be read/)).toBeInTheDocument();
  });
});
