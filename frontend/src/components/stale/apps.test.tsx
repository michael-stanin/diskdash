import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router";
import type { AppView } from "@/api/types/AppView";
import type { AppsResponse } from "@/api/types/AppsResponse";
import { ToastProvider } from "@/components/Toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AppsPage } from "@/pages/AppsPage";

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

// jsdom has no layout; give the table's scroll viewport a real size so the virtualiser renders rows.
beforeAll(() => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    width: 800, height: 440, top: 0, left: 0, right: 800, bottom: 440, x: 0, y: 0, toJSON: () => ({}),
  } as DOMRect);
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 440 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 800 });
});

beforeEach(() => {
  apiMock.mockReset();
});

describe("AppsPage", () => {
  const app = (over: Partial<AppView> = {}): AppView => ({
    name: "Foo",
    publisher: "Acme",
    version: "1.0",
    install_date: "2024-01-31",
    install_location: "D:\\Games\\Foo",
    size: 40 * 1024 ** 3,
    size_source: "scan",
    last_used: "2023-11-14T00:00:00Z",
    last_used_source: "userassist",
    sources: ["hklm64"],
    ...over,
  });
  const resp = (over: Partial<AppsResponse> = {}): AppsResponse => ({ apps: [app()], skipped: 0, prefetch_available: false, ...over });

  it("shows a skeleton while loading", () => {
    apiMock.mockReturnValue(new Promise(() => {}));
    wrap(<AppsPage />);
    expect(screen.getByLabelText("Loading applications")).toBeInTheDocument();
  });

  it("shows the registry error with retry", async () => {
    apiMock.mockRejectedValue(new Error("access denied"));
    wrap(<AppsPage />);
    expect(await screen.findByText("Cannot read the installed applications")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Retry/ })).toBeInTheDocument();
  });

  it("shows an empty state", async () => {
    apiMock.mockResolvedValue(resp({ apps: [] }));
    wrap(<AppsPage />);
    expect(await screen.findByText("No applications found")).toBeInTheDocument();
  });

  it("lists apps with size and last-used sources and a Windows Settings link only", async () => {
    apiMock.mockResolvedValue(resp({ skipped: 2 }));
    wrap(<AppsPage />);
    expect(await screen.findByText("Foo")).toBeInTheDocument();
    expect(screen.getByText("40 GiB")).toBeInTheDocument();
    expect(screen.getByText("From scan")).toBeInTheDocument();
    expect(screen.getByText("UserAssist")).toBeInTheDocument();
    expect(screen.getByText(/Prefetch data needs/)).toBeInTheDocument();
    expect(screen.getByText(/2 registry entries could not be read/)).toBeInTheDocument();
    const links = screen.getAllByRole("link", { name: /Apps & features/ });
    expect(links.length).toBeGreaterThanOrEqual(2);
    for (const l of links) expect(l).toHaveAttribute("href", "ms-settings:appsfeatures");
    expect(screen.queryByRole("button", { name: /uninstall/i })).not.toBeInTheDocument();
  });

  it("marks unknown sizes and last-used times", async () => {
    apiMock.mockResolvedValue(resp({ apps: [app({ size: null, size_source: "unknown", last_used: null, last_used_source: "unknown" })] }));
    wrap(<AppsPage />);
    await screen.findByText("Foo");
    expect(screen.getAllByText("unknown").length).toBeGreaterThanOrEqual(2);
  });
});
