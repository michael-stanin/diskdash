import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router";
import type { CandidatePage } from "@/api/types/CandidatePage";
import type { CandidateView } from "@/api/types/CandidateView";
import type { RulesView } from "@/api/types/RulesView";
import type { RuleView } from "@/api/types/RuleView";
import type { SnapshotCleanup } from "@/api/types/SnapshotCleanup";
import { ToastProvider } from "@/components/Toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { RulesPage } from "@/pages/RulesPage";
import { CandidateItem } from "./CandidateItem";
import { CandidateList } from "./CandidateList";
import { DriveSavings } from "./Savings";

const apiMock = vi.fn();
vi.mock("@/api/client", async (orig) => ({ ...(await orig<typeof import("@/api/client")>()), api: (...a: unknown[]) => apiMock(...a) }));
vi.mock("@/app/StatusCluster", () => ({ StatusCluster: () => null }));
const openScan = vi.fn();
vi.mock("@/components/scans/ScanDialog", () => ({ useScanDialog: () => ({ open: openScan }) }));

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

const candidate = (over: Partial<CandidateView> = {}): CandidateView => ({
  snapshot_id: 1,
  match_id: 10,
  root: "C:\\",
  rule_id: "windows-user-temp",
  title: "Your temporary files",
  category: "windows",
  explanation: "Scratch files that programs leave behind.",
  risk: "Safe",
  kind: "manual-cleanup",
  path: "C:\\Users\\Ann\\AppData\\Local\\Temp",
  is_dir: true,
  entry_id: 42,
  virtual_key: null,
  logical: 1000,
  allocated: 2 * 1024 * 1024 * 1024,
  file_count: 1234,
  modified: null,
  note: null,
  instructions: ["Open Settings > System > Storage.", "powercfg /h off"],
  nested: 0,
  ...over,
});

const page = (over: Partial<CandidatePage> = {}): CandidatePage => ({
  items: [],
  next_cursor: null,
  total: 0,
  total_allocated: 0,
  scanned: 1,
  evaluating: 0,
  failed: 0,
  error: null,
  ...over,
});

beforeEach(() => {
  apiMock.mockReset();
  openScan.mockReset();
});

describe("CandidateItem", () => {
  it("shows risk, size and path, and reveals copyable instructions", async () => {
    const user = userEvent.setup();
    wrap(<ul><CandidateItem item={candidate()} /></ul>);
    expect(screen.getByRole("heading", { name: "Your temporary files" })).toBeInTheDocument();
    expect(screen.getByText("Safe")).toBeInTheDocument();
    expect(screen.getByText("2 GiB")).toBeInTheDocument();
    expect(screen.getByText("C:\\Users\\Ann\\AppData\\Local\\Temp")).toBeInTheDocument();
    expect(screen.queryByText(/powercfg/)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /How to/ }));
    expect(screen.getByText("Open Settings > System > Storage.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Copy command powercfg /h off" }));
    await waitFor(async () => expect(await navigator.clipboard.readText()).toBe("powercfg /h off"));
    expect(screen.getByText(/only shows these steps/)).toBeInTheDocument();
  });

  it("offers Open in Explorer only for real files and folders", () => {
    const { unmount } = wrap(<ul><CandidateItem item={candidate()} /></ul>);
    expect(screen.getByRole("button", { name: /in Explorer/ })).toBeInTheDocument();
    unmount();
    wrap(<ul><CandidateItem item={candidate({ entry_id: null, virtual_key: "recycle-bin", path: "Recycle Bin (C:)" })} /></ul>);
    expect(screen.queryByRole("button", { name: /in Explorer/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Copy path/ })).toBeInTheDocument();
  });

  it("says when a size could not be measured and marks Windows settings", () => {
    wrap(
      <ul>
        <CandidateItem item={candidate({ allocated: null, kind: "system-setting", note: "Size unavailable without admin." })} />
      </ul>,
    );
    expect(screen.getByText("Size unknown")).toBeInTheDocument();
    expect(screen.getByText("Windows setting")).toBeInTheDocument();
    expect(screen.getByText("Size unavailable without admin.")).toBeInTheDocument();
  });

  it("never offers a delete, move or run action", () => {
    wrap(<ul><CandidateItem item={candidate()} /></ul>);
    for (const bad of [/delete/i, /remove now/i, /clean now/i, /move/i, /run/i]) {
      expect(screen.queryByRole("button", { name: bad })).not.toBeInTheDocument();
    }
  });
});

describe("CandidateList states", () => {
  it("shows a skeleton while loading", () => {
    apiMock.mockReturnValue(new Promise(() => {}));
    wrap(<CandidateList />);
    expect(screen.getByLabelText("Loading suggestions")).toBeInTheDocument();
  });

  it("asks for a scan when no drive was scanned", async () => {
    const user = userEvent.setup();
    apiMock.mockResolvedValue(page({ scanned: 0 }));
    wrap(<CandidateList />);
    expect(await screen.findByText("Scan a drive to see suggestions")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Scan a drive/ }));
    expect(openScan).toHaveBeenCalled();
  });

  it("says when there is nothing to suggest", async () => {
    apiMock.mockResolvedValue(page());
    wrap(<CandidateList scope="this drive" />);
    expect(await screen.findByText("Nothing to suggest for this drive")).toBeInTheDocument();
  });

  it("shows an error with retry", async () => {
    const user = userEvent.setup();
    apiMock.mockRejectedValueOnce(new Error("boom")).mockResolvedValue(page());
    wrap(<CandidateList />);
    expect(await screen.findByText("Cannot load suggestions")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Retry/ }));
    expect(await screen.findByText(/Nothing to suggest/)).toBeInTheDocument();
  });

  it("shows a failed evaluation with a Re-run action", async () => {
    const user = userEvent.setup();
    apiMock.mockImplementation((path: string) =>
      Promise.resolve(path.startsWith("/cleanup/evaluate") ? { id: 5 } : page({ failed: 1, error: "disk is full" })),
    );
    wrap(<CandidateList snapshotId={7} scope="this drive" />);
    expect(await screen.findByText("Could not check this scan against the rules")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Retry/ }));
    await waitFor(() =>
      expect(apiMock).toHaveBeenCalledWith("/cleanup/evaluate", expect.objectContaining({ method: "POST", body: { snapshot_ids: [7] } })),
    );
  });

  it("lists ranked suggestions, pages and filters by risk", async () => {
    const user = userEvent.setup();
    apiMock.mockImplementation((path: string) =>
      Promise.resolve(
        page({
          items: [candidate(), candidate({ match_id: 11, title: "Chrome cache", rule_id: "browser-chrome-cache", category: "browser" })],
          total: 3,
          total_allocated: 5 * 1024 ** 3,
          next_cursor: path.includes("cursor=0") ? 2 : null,
        }),
      ),
    );
    wrap(<CandidateList />);
    expect(await screen.findByRole("heading", { name: "Chrome cache" })).toBeInTheDocument();
    expect(screen.getByText(/3 suggestions/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Show more" })).toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: "Safe" }));
    await waitFor(() => expect(apiMock.mock.calls.some(([p]) => String(p).includes("risk=Safe"))).toBe(true));
  });
});

describe("DriveSavings", () => {
  const done = (savings: SnapshotCleanup["savings"]): SnapshotCleanup => ({
    run: { snapshot_id: 1, status: "complete", error: null, finished_at: null, rule_errors: 0, match_count: 3, stale: false },
    savings,
  });

  it("renders nothing without a run", () => {
    const { container } = wrap(<DriveSavings cleanup={{ run: null, savings: [] }} />);
    expect(container.querySelector("[aria-label='Potential savings']")).toBeNull();
  });

  it("shows progress and failure", () => {
    const run = (status: "queued" | "failed"): SnapshotCleanup => ({
      run: { snapshot_id: 1, status, error: status === "failed" ? "boom" : null, finished_at: null, rule_errors: 0, match_count: 0, stale: false },
      savings: [],
    });
    const { unmount } = wrap(<DriveSavings cleanup={run("queued")} />);
    expect(screen.getByText("Looking for things to clean up…")).toBeInTheDocument();
    unmount();
    wrap(<DriveSavings cleanup={run("failed")} />);
    expect(screen.getByText(/Could not check this scan for savings \(boom\)/)).toBeInTheDocument();
  });

  it("splits savings by risk and totals Windows settings separately", () => {
    const gib = 1024 ** 3;
    wrap(
      <DriveSavings
        cleanup={done([
          { risk: "Safe", kind: "manual-cleanup", allocated: 3 * gib, logical: 3 * gib, matches: 4, unknown: 0 },
          { risk: "Review", kind: "manual-cleanup", allocated: 10 * gib, logical: 10 * gib, matches: 2, unknown: 0 },
          { risk: "Review", kind: "system-setting", allocated: 12 * gib, logical: 12 * gib, matches: 1, unknown: 0 },
        ])}
      />,
    );
    const box = screen.getByLabelText("Potential savings");
    expect(within(box).getByText("3 GiB")).toBeInTheDocument();
    expect(within(box).getByText("10 GiB")).toBeInTheDocument();
    expect(within(box).getByText("12 GiB")).toBeInTheDocument();
    expect(within(box).getByText(/held by Windows settings/)).toBeInTheDocument();
  });

  it("says when nothing was found", () => {
    wrap(<DriveSavings cleanup={done([])} />);
    expect(screen.getByText("Nothing to clean up found in the last scan.")).toBeInTheDocument();
  });
});

describe("RulesPage", () => {
  const rule = (over: Partial<RuleView> = {}): RuleView => ({
    id: "windows-user-temp",
    title: "Your temporary files",
    category: "windows",
    explanation: "Scratch files.",
    risk: "Safe",
    kind: "manual-cleanup",
    source: { type: "builtin" },
    overrides_builtin: false,
    disabled: false,
    expanded_paths: [{ template: "%TEMP%", path: "C:\\Users\\Ann\\AppData\\Local\\Temp", error: null }],
    globs: [],
    min_age_days: null,
    applies_to: null,
    conditions: { extracted_sibling: false },
    virtual_source: null,
    instructions: ["Delete the files in Explorer."],
    ...over,
  });
  const view = (over: Partial<RulesView> = {}): RulesView => ({
    rules: [rule()],
    issues: [],
    user_dir: { path: "C:\\Users\\Ann\\AppData\\Local\\diskdash\\rules", exists: true },
    user_files: 0,
    ...over,
  });

  it("shows a skeleton while loading", () => {
    apiMock.mockReturnValue(new Promise(() => {}));
    wrap(<RulesPage />);
    expect(screen.getByLabelText("Loading rules")).toBeInTheDocument();
  });

  it("shows an error with retry", async () => {
    apiMock.mockRejectedValue(new Error("nope"));
    wrap(<RulesPage />);
    expect(await screen.findByText("Cannot load the rules")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Retry/ })).toBeInTheDocument();
  });

  it("explains how to add rules when there are none of your own", async () => {
    apiMock.mockResolvedValue(view());
    wrap(<RulesPage />);
    expect(await screen.findByText(/You have no rules of your own yet/)).toBeInTheDocument();
    expect(screen.getByText("C:\\Users\\Ann\\AppData\\Local\\diskdash\\rules")).toBeInTheDocument();
    expect(screen.getByText("Built-in")).toBeInTheDocument();
  });

  it("lists problems with file and line, and marks overridden and disabled rules", async () => {
    apiMock.mockResolvedValue(
      view({
        issues: [{ file: "broken.yaml", rule_id: null, index: null, line: 3, column: 4, message: "invalid YAML: did not find expected key" }],
        rules: [
          rule({ id: "a", title: "Mine", overrides_builtin: true, source: { type: "user", file: "mine.yaml" } }),
          rule({ id: "b", title: "Prefetch", disabled: true, risk: "Expert" }),
        ],
        user_files: 2,
      }),
    );
    wrap(<RulesPage />);
    const alert = await screen.findByRole("alert", { name: "Rule problems" });
    expect(within(alert).getByText("broken.yaml:3:4")).toBeInTheDocument();
    expect(within(alert).getByText(/invalid YAML/)).toBeInTheDocument();
    expect(screen.getByText("Overridden by you")).toBeInTheDocument();
    expect(screen.getByText("Disabled")).toBeInTheDocument();
    expect(screen.queryByText(/You have no rules of your own yet/)).not.toBeInTheDocument();
  });

  it("shows where a rule looks, with unresolved variables flagged", async () => {
    const user = userEvent.setup();
    apiMock.mockResolvedValue(
      view({
        rules: [
          rule({
            expanded_paths: [
              { template: "%TEMP%", path: "C:\\Temp", error: null },
              { template: "%NOPE%\\x", path: null, error: "environment variable %NOPE% is not set" },
            ],
            conditions: { sibling_exists: "package.json", extracted_sibling: false },
            min_age_days: 30,
          }),
        ],
      }),
    );
    wrap(<RulesPage />);
    await user.click(await screen.findByRole("button", { name: /Details/ }));
    expect(screen.getByText("C:\\Temp")).toBeInTheDocument();
    expect(screen.getByText(/%NOPE%\\x — environment variable/)).toBeInTheDocument();
    expect(screen.getByText("a file named package.json sits next to it")).toBeInTheDocument();
    expect(screen.getByText("nothing in it changed for 30 days")).toBeInTheDocument();
  });
});
