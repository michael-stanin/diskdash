import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import type { DupGroup } from "@/api/types/DupGroup";
import type { DupGroupsPage } from "@/api/types/DupGroupsPage";
import type { DupRun } from "@/api/types/DupRun";
import { ToastProvider } from "@/components/Toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { DuplicatesView, REMOVE_STEPS } from "./DuplicatesView";

const apiMock = vi.fn();
vi.mock("@/api/client", async (orig) => ({ ...(await orig<typeof import("@/api/client")>()), api: (...a: unknown[]) => apiMock(...a) }));
let jobs: unknown[] = [];
vi.mock("@/api/events", () => ({ useEvents: () => ({ activeJobs: jobs, state: "open" }) }));

function wrap(ui: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <ToastProvider>{ui}</ToastProvider>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const run = (over: Partial<DupRun> = {}): DupRun => ({
  id: 1,
  status: "complete",
  algo: "blake3",
  min_size: 1024 * 1024,
  roots: ["C:\\"],
  near_enabled: false,
  started_at: "2026-10-15T10:00:00Z",
  finished_at: "2026-10-15T10:05:00Z",
  error: null,
  job_id: 9,
  candidates: 120,
  files_hashed: 40,
  cache_hits: 10,
  group_count: 1,
  wasted: 6 * 1024 ** 2,
  skips: {},
  near_truncated: false,
  atime_roots: [],
  ...over,
});

const group = (over: Partial<DupGroup> = {}): DupGroup => ({
  id: 5,
  hash: "ab".repeat(32),
  size: 3 * 1024 ** 2,
  alloc: 3 * 1024 ** 2,
  wasted: 6 * 1024 ** 2,
  rule: null,
  members: [
    { id: 1, snapshot_id: 1, entry_id: 11, path: "C:\\photos\\a.jpg", drive: "C", modified: "2024-01-01T00:00:00Z", category: "images", suggested: null },
    { id: 2, snapshot_id: 1, entry_id: 12, path: "C:\\backup\\a.jpg", drive: "C", modified: "2023-01-01T00:00:00Z", category: "images", suggested: null },
    { id: 3, snapshot_id: 1, entry_id: 13, path: "C:\\old\\a.jpg", drive: "C", modified: "2022-01-01T00:00:00Z", category: "images", suggested: null },
  ],
  ...over,
});

const page = (over: Partial<DupGroupsPage> = {}): DupGroupsPage => ({
  groups: [group()],
  next_cursor: null,
  total: 1,
  total_wasted: 6 * 1024 ** 2,
  run: run(),
  ...over,
});

function serve(opts: { status?: DupRun | null; groups?: DupGroupsPage; snapshots?: unknown[] }) {
  apiMock.mockImplementation((path: string) => {
    if (path.startsWith("/duplicates/status")) return Promise.resolve(opts.status === undefined ? run() : opts.status);
    if (path.startsWith("/duplicates/groups")) return Promise.resolve(opts.groups ?? page());
    if (path.startsWith("/snapshots/latest")) return Promise.resolve(opts.snapshots ?? [{ id: 1, root: "C:\\", time_basis: "mtime" }]);
    if (path.startsWith("/duplicates/runs")) return Promise.resolve({ id: 3 });
    if (path.startsWith("/duplicates/suggestions")) return Promise.resolve({ applied: 1 });
    if (path.startsWith("/duplicates/skips")) return Promise.resolve([{ reason: "sharing-violation", path: "C:\\locked.bin" }]);
    return Promise.resolve(null);
  });
}

beforeEach(() => {
  apiMock.mockReset();
  jobs = [];
});

describe("DuplicatesView", () => {
  it("shows a skeleton, then explains what will be read when never run, with a note about last-access", async () => {
    serve({ status: null, snapshots: [{ id: 1, root: "D:\\", time_basis: "atime" }] });
    wrap(<DuplicatesView />);
    expect(screen.getByLabelText("Loading duplicates")).toBeInTheDocument();
    expect(await screen.findByText("Find duplicate files")).toBeInTheDocument();
    expect(screen.getByText(/It only reads/)).toBeInTheDocument();
    expect(await screen.findByRole("note")).toHaveTextContent(/D:.*last-access times/);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /Find duplicates/ }));
    await waitFor(() =>
      expect(apiMock).toHaveBeenCalledWith("/duplicates/runs", expect.objectContaining({ method: "POST", body: expect.objectContaining({ min_size_mib: 1, near_duplicates: false }) })),
    );
  });

  it("needs a scan before it can start", async () => {
    serve({ status: null, snapshots: [] });
    wrap(<DuplicatesView />);
    expect(await screen.findByText(/Scan a drive first/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Find duplicates/ })).toBeDisabled();
  });

  it("shows an error with retry", async () => {
    const user = userEvent.setup();
    apiMock.mockRejectedValueOnce(new Error("boom"));
    serve({});
    apiMock.mockRejectedValueOnce(new Error("boom"));
    wrap(<DuplicatesView />);
    expect(await screen.findByText("Cannot load the duplicate search")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Retry/ }));
  });

  it("shows progress with a cancel control while running", async () => {
    const user = userEvent.setup();
    jobs = [{ id: 9, kind: "duplicates", status: "running", phase: "Reading files to compare them", done: 50 * 1024 ** 2, total: 200 * 1024 ** 2, eta_seconds: 90 }];
    serve({ status: run({ status: "running", job_id: 9 }) });
    wrap(<DuplicatesView />);
    expect(await screen.findByLabelText("Duplicate search in progress")).toBeInTheDocument();
    expect(screen.getByText(/50 MiB of 200 MiB/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel the duplicate search" }));
    await waitFor(() => expect(apiMock).toHaveBeenCalledWith("/jobs/9/cancel", expect.objectContaining({ method: "POST" })));
  });

  it("lists groups with wasted space and expands to the copies with copy and Explorer actions only", async () => {
    const user = userEvent.setup();
    serve({});
    wrap(<DuplicatesView />);
    const item = await screen.findByRole("listitem", { name: "Duplicates of a.jpg" });
    expect(within(item).getByText(/3 copies of/)).toBeInTheDocument();
    expect(screen.getByText(/^1 group/)).toBeInTheDocument();
    await user.click(within(item).getByRole("button", { expanded: false }));
    expect(screen.getByText("C:\\backup\\a.jpg")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /in Explorer/ })).toHaveLength(3);
    for (const bad of [/delete/i, /remove/i, /hard.?link/i]) expect(screen.queryByRole("button", { name: bad })).not.toBeInTheDocument();
  });

  it("applies a keep rule and shows keep/remove with manual steps", async () => {
    const user = userEvent.setup();
    serve({});
    wrap(<DuplicatesView />);
    await screen.findByRole("listitem", { name: "Duplicates of a.jpg" });
    await user.selectOptions(screen.getByLabelText("Keep rule"), "oldest");
    await user.click(screen.getByRole("button", { name: "Suggest which to keep" }));
    await waitFor(() =>
      expect(apiMock).toHaveBeenCalledWith("/duplicates/suggestions", expect.objectContaining({ method: "POST", body: expect.objectContaining({ rule: "oldest" }) })),
    );
    // With suggestions in place the group shows who stays and who is for the user to remove.
    serve({
      groups: page({
        groups: [group({ rule: "oldest", members: group().members.map((m) => ({ ...m, suggested: m.id === 3 ? "keep" : "remove" })) })],
      }),
    });
  });

  it("shows keep and remove badges with the manual steps", async () => {
    const user = userEvent.setup();
    serve({ groups: page({ groups: [group({ rule: "oldest", members: group().members.map((m) => ({ ...m, suggested: m.id === 3 ? "keep" : "remove" })) })] }) });
    wrap(<DuplicatesView />);
    const item = await screen.findByRole("listitem", { name: "Duplicates of a.jpg" });
    await user.click(within(item).getByRole("button", { expanded: false }));
    expect(screen.getAllByText("Keep")).toHaveLength(1);
    expect(screen.getAllByText("Remove")).toHaveLength(2);
    expect(screen.getAllByText(REMOVE_STEPS)).toHaveLength(2);
  });

  it("prefer rules need a value", async () => {
    const user = userEvent.setup();
    serve({});
    wrap(<DuplicatesView />);
    await screen.findByRole("listitem", { name: "Duplicates of a.jpg" });
    await user.selectOptions(screen.getByLabelText("Keep rule"), "prefer-drive");
    expect(screen.getByRole("button", { name: "Suggest which to keep" })).toBeDisabled();
    await user.type(screen.getByLabelText("Drive letter"), "D");
    expect(screen.getByRole("button", { name: "Suggest which to keep" })).toBeEnabled();
  });

  it("says when no duplicates were found, with the scope and date", async () => {
    serve({ groups: page({ groups: [], total: 0, total_wasted: 0 }), status: run({ group_count: 0, wasted: 0 }) });
    wrap(<DuplicatesView />);
    expect(await screen.findByText("No duplicates over 1 MiB found")).toBeInTheDocument();
    expect(screen.getAllByText(/Searched C:\\/).length).toBeGreaterThan(0);
  });

  it("offers Show more for further pages", async () => {
    serve({ groups: page({ next_cursor: 20 }) });
    wrap(<DuplicatesView />);
    expect(await screen.findByRole("button", { name: "Show more" })).toBeInTheDocument();
  });

  it("shows a failed or cancelled search with a way to search again", async () => {
    serve({ status: run({ status: "failed", error: "disk is gone" }) });
    wrap(<DuplicatesView />);
    expect(await screen.findByText("The duplicate search failed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Search again/ })).toBeInTheDocument();
  });

  it("lists what was left out, including the files themselves for failures", async () => {
    const user = userEvent.setup();
    serve({ status: run({ skips: { "cloud-placeholder": 3, "sharing-violation": 1 } }) });
    wrap(<DuplicatesView />);
    await user.click(await screen.findByText(/Left out: 4 files/));
    expect(screen.getByText(/Online-only cloud files \(never opened\)/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Show files" }));
    expect(await screen.findByText("C:\\locked.bin")).toBeInTheDocument();
  });
});
