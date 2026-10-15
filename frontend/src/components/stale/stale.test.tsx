import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router";
import type { EntryView } from "@/api/types/EntryView";
import type { Snapshot } from "@/api/types/Snapshot";
import type { StaleItem } from "@/api/types/StaleItem";
import type { StalePageView } from "@/api/types/StalePageView";
import { ToastProvider } from "@/components/Toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { FSUTIL_COMMAND, StaleTab, TimeBasisBanner } from "./StaleTab";

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

const snapshot = (over: Partial<Snapshot> = {}): Snapshot => ({ id: 7, time_basis: "mtime", time_basis_reason: "disabled", ...over }) as Snapshot;

const entry = (over: Partial<EntryView> = {}): EntryView =>
  ({
    id: 1,
    name: "old",
    path: "D:\\archive\\old",
    kind: "dir",
    size: 1,
    alloc: 5 * 1024 ** 3,
    category: null,
    top_category: "video",
    file_count: 120,
    access_denied: false,
    ...over,
  }) as EntryView;

const item = (over: Partial<StaleItem> = {}): StaleItem => ({ entry: entry(), time: "2022-01-01T00:00:00Z", age_days: 1400, score: null, ...over });

const page = (over: Partial<StalePageView> = {}): { data: StalePageView } => ({
  data: { items: [], next_cursor: null, time_basis: "mtime", time_basis_reason: "disabled", months: 12, excluded_invalid: 0, ...over },
});

beforeEach(() => {
  apiMock.mockReset();
});

describe("TimeBasisBanner", () => {
  it("explains the fallback and offers the command as copyable text", async () => {
    const user = userEvent.setup();
    wrap(<TimeBasisBanner snapshot={snapshot()} />);
    expect(screen.getByText(/Using last-modified time/)).toBeInTheDocument();
    expect(screen.getByText(FSUTIL_COMMAND)).toBeInTheDocument();
    expect(screen.getByText(/diskdash never runs it/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Copy the fsutil command" }));
    await waitFor(async () => expect(await navigator.clipboard.readText()).toBe(FSUTIL_COMMAND));
  });

  it("says when last-access time is trusted and shows no command", () => {
    wrap(<TimeBasisBanner snapshot={snapshot({ time_basis: "atime", time_basis_reason: "enabled" })} />);
    expect(screen.getByText(/last access/)).toBeInTheDocument();
    expect(screen.queryByText(FSUTIL_COMMAND)).not.toBeInTheDocument();
  });

  it("says when reliability is unknown", () => {
    wrap(<TimeBasisBanner snapshot={snapshot({ time_basis_reason: "unknown" })} />);
    expect(screen.getByText(/reliability unknown/)).toBeInTheDocument();
  });
});

describe("StaleTab", () => {
  it("shows a skeleton, then ranked rows with age and size", async () => {
    apiMock.mockResolvedValue(page({ items: [item()], excluded_invalid: 3 }));
    wrap(<StaleTab snapshot={snapshot()} />);
    expect(screen.getByLabelText("Loading")).toBeInTheDocument();
    expect(await screen.findByText("D:\\archive\\old")).toBeInTheDocument();
    expect(screen.getByText("5 GiB")).toBeInTheDocument();
    expect(screen.getByText(/3\.8 years/)).toBeInTheDocument();
    expect(screen.getByText("3 items without valid timestamps excluded")).toBeInTheDocument();
    expect(apiMock.mock.calls[0]![0]).toContain("months=12");
  });

  it("re-queries with another preset and with a custom value", async () => {
    const user = userEvent.setup();
    apiMock.mockResolvedValue(page({ items: [item()] }));
    wrap(<StaleTab snapshot={snapshot()} />);
    await screen.findByText("D:\\archive\\old");
    await user.click(screen.getByRole("radio", { name: "24 months" }));
    await waitFor(() => expect(apiMock.mock.calls.some(([p]) => String(p).includes("months=24"))).toBe(true));
    await user.type(screen.getByLabelText("Custom number of months"), "18");
    await waitFor(() => expect(apiMock.mock.calls.some(([p]) => String(p).includes("months=18"))).toBe(true));
  });

  it("offers a shorter preset when nothing is old enough", async () => {
    const user = userEvent.setup();
    apiMock.mockResolvedValue(page());
    wrap(<StaleTab snapshot={snapshot()} />);
    expect(await screen.findByText("Nothing older than 12 months on this drive")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Try 6 months" }));
    expect(await screen.findByText("Nothing older than 6 months on this drive")).toBeInTheDocument();
  });

  it("shows an error with retry", async () => {
    const user = userEvent.setup();
    apiMock.mockRejectedValueOnce(new Error("boom")).mockResolvedValue(page({ items: [item()] }));
    wrap(<StaleTab snapshot={snapshot()} />);
    expect(await screen.findByText("Cannot load this list")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Retry/ }));
    expect(await screen.findByText("D:\\archive\\old")).toBeInTheDocument();
  });

  it("lists old large files with their score", async () => {
    const user = userEvent.setup();
    apiMock.mockImplementation((p: string) =>
      Promise.resolve(p.includes("old-large") ? page({ items: [item({ entry: entry({ kind: "file", name: "x.iso", path: "D:\\x.iso" }), score: 400 * 1024 ** 3 })] }) : page()),
    );
    wrap(<StaleTab snapshot={snapshot()} />);
    await user.click(screen.getByRole("radio", { name: "Old large files" }));
    expect(await screen.findByText("D:\\x.iso")).toBeInTheDocument();
    expect(screen.getByText(/score 400 GiB·days/)).toBeInTheDocument();
  });

  it("never offers a delete or run action", async () => {
    apiMock.mockResolvedValue(page({ items: [item()] }));
    wrap(<StaleTab snapshot={snapshot()} />);
    await screen.findByText("D:\\archive\\old");
    for (const bad of [/delete/i, /remove/i, /archive it/i, /run/i]) {
      expect(screen.queryByRole("button", { name: bad })).not.toBeInTheDocument();
    }
  });
});

