import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { MemoryRouter } from "react-router";
import { ApiError } from "@/api/client";
import type { PlanItemView } from "@/api/types/PlanItemView";
import type { PlanSummary } from "@/api/types/PlanSummary";
import type { PlanView } from "@/api/types/PlanView";
import { PathList } from "@/components/settings/PathList";
import { ToastProvider } from "@/components/Toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { PlansPage } from "@/pages/PlansPage";
import { AddToPlanButton, protectionText } from "./AddToPlan";

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

const bucket = (count = 0, allocated = 0) => ({ count, logical: allocated, allocated });
const summary = (over: Partial<PlanSummary> = {}): PlanSummary => ({
  id: 1,
  name: "Spring clean",
  created_at: "2026-10-15T10:00:00Z",
  updated_at: "2026-10-15T10:00:00Z",
  totals: { overall: bucket(2, 3 * 1024 ** 3), todo: bucket(1, 1024 ** 3), done: bucket(1, 2 * 1024 ** 3), skipped: bucket(), by_drive: [{ drive: "C:", count: 2, logical: 3 * 1024 ** 3, allocated: 3 * 1024 ** 3 }], unknown_size: 0 },
  blocked: 0,
  ...over,
});

const item = (over: Partial<PlanItemView> = {}): PlanItemView => ({
  id: 10,
  snapshot_id: 1,
  entry_id: 5,
  path: "C:\\proj\\node_modules",
  drive: "C:",
  is_dir: true,
  kind: "manual-cleanup",
  source: "cleanup-rules",
  title: "node_modules folders",
  category: "developer",
  risk: "Review",
  logical: 1024 ** 3,
  allocated: 1024 ** 3,
  destination: null,
  instructions: ["Delete the node_modules folder in Explorer.", "npm install"],
  status: "todo",
  note: null,
  added_at: "2026-10-15T10:00:00Z",
  blocked: null,
  verify: { status: "unverified", freed: null, verified_at: null },
  ...over,
});

const view = (over: Partial<PlanView> = {}, items: PlanItemView[] = [item()]): PlanView => ({
  plan: summary(),
  items,
  verifications: [],
  freed_total: 0,
  baseline_as_of: "2026-10-14T10:00:00Z",
  ...over,
});

function serve(opts: { plans?: PlanSummary[]; plan?: PlanView }) {
  apiMock.mockImplementation((path: string, o?: { method?: string; body?: unknown }) => {
    if (path === "/plans" && !o?.method) return Promise.resolve(opts.plans ?? [summary()]);
    if (path === "/plans" && o?.method === "POST") return Promise.resolve(summary({ id: 2, name: String((o.body as { name: string }).name) }));
    if (/^\/plans\/\d+$/.test(path) && !o?.method) return Promise.resolve(opts.plan ?? view());
    if (path.endsWith("/items") && o?.method === "POST") return Promise.resolve({ added: true, reason: null, item: item() });
    if (path.includes("/items/") && o?.method === "PATCH") return Promise.resolve(item());
    if (path.endsWith("/verify")) return Promise.resolve({ id: 7 });
    if (path.startsWith("/plans/") && o?.method === "DELETE") return Promise.resolve(null);
    return Promise.resolve(null);
  });
}

beforeEach(() => {
  apiMock.mockReset();
  localStorage.clear();
});

describe("AddToPlanButton", () => {
  it("adds to an existing plan and says nothing is changed by it", async () => {
    const user = userEvent.setup();
    serve({});
    wrap(<AddToPlanButton what={"C:\\proj\\node_modules"} body={{ source: "cleanup", snapshot_id: 1, match_id: 3 }} />);
    expect(apiMock).not.toHaveBeenCalled(); // plans load only when the dialog opens
    await user.click(screen.getByRole("button", { name: /Add to plan: C:\\proj\\node_modules/ }));
    expect(await screen.findByText(/does not change it in any way/)).toBeInTheDocument();
    expect(await screen.findByText("Spring clean")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() =>
      expect(apiMock).toHaveBeenCalledWith("/plans/1/items", expect.objectContaining({ method: "POST", body: { source: "cleanup", snapshot_id: 1, match_id: 3 } })),
    );
    expect(await screen.findByText("Added to Spring clean")).toBeInTheDocument();
  });

  it("creates a plan when there is none", async () => {
    const user = userEvent.setup();
    serve({ plans: [] });
    wrap(<AddToPlanButton what="x" body={{ source: "stale", snapshot_id: 1, entry_id: 4 }} />);
    await user.click(screen.getByRole("button", { name: /Add to plan/ }));
    await user.type(await screen.findByLabelText("New plan name"), "Fresh start");
    await user.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() => expect(apiMock).toHaveBeenCalledWith("/plans", expect.objectContaining({ method: "POST", body: { name: "Fresh start" } })));
    await waitFor(() => expect(apiMock).toHaveBeenCalledWith("/plans/2/items", expect.anything()));
  });

  it("reports an item that is already planned", async () => {
    const user = userEvent.setup();
    serve({});
    apiMock.mockImplementationOnce(() => Promise.resolve([summary()]));
    wrap(<AddToPlanButton what="x" body={{ source: "duplicate", member_id: 1 }} />);
    await user.click(screen.getByRole("button", { name: /Add to plan/ }));
    await screen.findByText("Spring clean");
    apiMock.mockImplementation((p: string) => Promise.resolve(p.endsWith("/items") ? { added: false, reason: "already-planned", item: item() } : [summary()]));
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(await screen.findByText("Already in Spring clean")).toBeInTheDocument();
  });

  it("explains a protected location in plain language", async () => {
    const user = userEvent.setup();
    serve({});
    wrap(<AddToPlanButton what={"C:\\Program Files\\App"} body={{ source: "stale", snapshot_id: 1, entry_id: 9 }} />);
    await user.click(screen.getByRole("button", { name: /Add to plan/ }));
    await screen.findByText("Spring clean");
    apiMock.mockImplementation((p: string) => {
      if (p.endsWith("/items")) return Promise.reject(new ApiError(422, { error: "validation", message: "validation failed", fields: { path: "protected:program_files" } }, "x"));
      return Promise.resolve([summary()]);
    });
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/Program Files.*never suggested or planned/);
    expect(protectionText("excluded:user")).toMatch(/folder you excluded/);
  });
});

describe("PlansPage", () => {
  it("shows a skeleton while loading", () => {
    apiMock.mockReturnValue(new Promise(() => {}));
    wrap(<PlansPage />);
    expect(screen.getByLabelText("Loading plans")).toBeInTheDocument();
  });

  it("shows an error with retry", async () => {
    apiMock.mockRejectedValue(new Error("boom"));
    wrap(<PlansPage />);
    expect(await screen.findByText("Cannot load your plans")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Retry/ })).toBeInTheDocument();
  });

  it("explains plans and links to Recommendations when there are none", async () => {
    serve({ plans: [] });
    wrap(<PlansPage />);
    expect(await screen.findByText("No plans yet")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open Recommendations" })).toHaveAttribute("href", "/recommendations");
  });

  it("shows totals, items, steps and export links, and tracks status", async () => {
    const user = userEvent.setup();
    serve({});
    wrap(<PlansPage />);
    expect(await screen.findByRole("heading", { name: "Spring clean" })).toBeInTheDocument();
    const totals = await screen.findByLabelText("Plan totals");
    expect(totals).toHaveTextContent(/Itemss*2/);
    expect(within(totals).getByText(/1 done · 1 to do · 0 skipped/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Export the plan as CSV" })).toHaveAttribute("href", "/api/v1/plans/1/export?format=csv");
    expect(screen.getByRole("link", { name: "Export the plan as a text checklist" })).toHaveAttribute("href", "/api/v1/plans/1/export?format=txt");
    expect(screen.getByText(/never changes anything/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Steps/ }));
    expect(screen.getByText("Delete the node_modules folder in Explorer.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy command npm install" })).toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: "Done" }));
    await waitFor(() => expect(apiMock).toHaveBeenCalledWith("/plans/1/items/10", expect.objectContaining({ method: "PATCH", body: { status: "done", note: undefined } })));
    const rows = within(screen.getByLabelText("Plan items"));
    for (const bad of [/^delete/i, /run/i, /execute/i]) expect(rows.queryByRole("button", { name: bad })).not.toBeInTheDocument();
  });

  it("shows what verification found per item and for the plan", async () => {
    const items = [
      item({ id: 1, title: "Removed one", verify: { status: "removed", freed: 1024 ** 3, verified_at: "2026-10-15T11:00:00Z" } }),
      item({ id: 2, title: "Still there", path: "C:\\b", verify: { status: "present", freed: 0, verified_at: "2026-10-15T11:00:00Z" } }),
      item({ id: 3, title: "Bigger now", path: "C:\\c", verify: { status: "grown", freed: -500 * 1024 ** 2, verified_at: "2026-10-15T11:00:00Z" } }),
    ];
    serve({
      plan: view(
        { freed_total: 1024 ** 3, verifications: [{ root: "C:\\", snapshot_id: 4, verified_at: "2026-10-15T11:00:00Z", items: 3, freed: 1024 ** 3, scanned_change: 2 * 1024 ** 3 }] },
        items,
      ),
    });
    wrap(<PlansPage />);
    expect(await screen.findByText(/Removed · 1 GiB freed/)).toBeInTheDocument();
    expect(screen.getByText(/Still present · 0 B freed/)).toBeInTheDocument();
    expect(screen.getByText(/Grew by 500 MiB/)).toBeInTheDocument();
    const v = screen.getByLabelText("Verification");
    expect(v).toHaveTextContent(/1 GiB freed by 3 planned items/);
    expect(v).toHaveTextContent(/Scanned size changed by -2 GiB \(includes unrelated activity\)/);
  });

  it("flags items that are now excluded and shows moves with their destination", async () => {
    serve({ plan: view({}, [item({ blocked: "excluded:user" }), item({ id: 11, path: "D:\\Games\\G", title: "Move G", kind: "manual-move", destination: "E:\\Games" })]) });
    wrap(<PlansPage />);
    expect(await screen.findByText("Now excluded")).toBeInTheDocument();
    expect(screen.getByText("E:\\Games")).toBeInTheDocument();
  });

  it("shows an empty plan and confirms that deleting a plan leaves files alone", async () => {
    const user = userEvent.setup();
    serve({ plan: view({}, []) });
    wrap(<PlansPage />);
    expect(await screen.findByText("This plan is empty")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete the plan" }));
    expect(await screen.findByText(/Your files are not touched/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete plan" }));
    await waitFor(() => expect(apiMock).toHaveBeenCalledWith("/plans/1", expect.objectContaining({ method: "DELETE" })));
  });

  it("shows an error for one plan while the others stay selectable", async () => {
    apiMock.mockImplementation((p: string) => {
      if (p === "/plans") return Promise.resolve([summary(), summary({ id: 2, name: "Other" })]);
      if (p === "/plans/1") return Promise.reject(new Error("server error"));
      return Promise.resolve(view({ plan: summary({ id: 2, name: "Other" }) }));
    });
    const user = userEvent.setup();
    wrap(<PlansPage />);
    expect(await screen.findByText("Cannot load this plan")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Other/ }));
    expect(await screen.findByRole("heading", { name: "Other" })).toBeInTheDocument();
  });
});

describe("PathList", () => {
  function Harness({ start = [] as string[] }) {
    const [v, setV] = useState(start);
    return <PathList id="p" label="Excluded folders" hint="h" value={v} onChange={setV} emptyText="No exclusions yet." />;
  }

  it("adds, validates and removes folders", async () => {
    const user = userEvent.setup();
    wrap(<Harness />);
    expect(screen.getByText("No exclusions yet.")).toBeInTheDocument();
    await user.type(screen.getByRole("textbox"), "not a path");
    await user.click(screen.getByRole("button", { name: /Add/ }));
    expect(screen.getByRole("alert")).toHaveTextContent(/full folder path/);
    await user.clear(screen.getByRole("textbox"));
    await user.type(screen.getByRole("textbox"), "D:\\Photos{Enter}");
    expect(screen.getByText("D:\\Photos")).toBeInTheDocument();
    await user.type(screen.getByRole("textbox"), "d:\\photos{Enter}");
    expect(screen.getByRole("alert")).toHaveTextContent(/already in the list/);
    await user.click(screen.getByRole("button", { name: "Remove D:\\Photos" }));
    expect(screen.getByText("No exclusions yet.")).toBeInTheDocument();
  });
});
