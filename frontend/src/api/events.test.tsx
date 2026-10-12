import { act, render, screen } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { backoff, EventsProvider, useEvents } from "./events";
import { createQueryClient } from "./queries";

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  static readonly CLOSED = 2;
  readyState = 0;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  listeners = new Map<string, (e: MessageEvent<string>) => void>();
  constructor(public url: string) {
    FakeEventSource.instances.push(this);
  }
  addEventListener(type: string, fn: (e: MessageEvent<string>) => void) {
    this.listeners.set(type, fn);
  }
  close() {
    this.readyState = 2;
  }
  emit(type: string, data: unknown) {
    this.listeners.get(type)?.({ data: JSON.stringify(data) } as MessageEvent<string>);
  }
}

function Probe() {
  const { state, activeJobs } = useEvents();
  return (
    <div>
      <span data-testid="state">{state}</span>
      <span data-testid="jobs">{activeJobs.map((j) => `${j.id}:${j.status}`).join(",")}</span>
    </div>
  );
}

function renderProvider() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <EventsProvider>
        <Probe />
      </EventsProvider>
    </QueryClientProvider>,
  );
}

describe("EventsProvider", () => {
  beforeEach(() => {
    FakeEventSource.instances = [];
    vi.stubGlobal("EventSource", FakeEventSource);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("is connected once health and the stream are up, and tracks jobs", async () => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify({ status: "ok", version: "0" })))));
    renderProvider();
    const es = FakeEventSource.instances[0]!;
    act(() => es.onopen?.());
    expect(await screen.findByText("connected")).toBeInTheDocument();

    const job = { id: 7, kind: "scan", status: "running" };
    act(() => es.emit("job.started", { id: 1, type: "job.started", data: job }));
    expect(screen.getByTestId("jobs")).toHaveTextContent("7:running");
    act(() => es.emit("job.finished", { id: 2, type: "job.finished", data: { ...job, status: "succeeded" } }));
    expect(screen.getByTestId("jobs")).toHaveTextContent("");
  });

  it("shows disconnected when the backend is unreachable", async () => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new TypeError("Failed to fetch"))));
    renderProvider();
    expect(await screen.findByText("disconnected")).toBeInTheDocument();
  });

  it("rebuilds a closed stream with backoff", () => {
    vi.useFakeTimers();
    vi.stubGlobal("fetch", vi.fn(() => new Promise(() => {})));
    renderProvider();
    const first = FakeEventSource.instances[0]!;
    first.readyState = FakeEventSource.CLOSED;
    act(() => first.onerror?.());
    expect(FakeEventSource.instances).toHaveLength(1);
    act(() => vi.advanceTimersByTime(1_000));
    expect(FakeEventSource.instances).toHaveLength(2);
  });

  it("backoff grows and is capped", () => {
    expect([0, 1, 2, 3, 10].map(backoff)).toEqual([1000, 2000, 4000, 8000, 15000]);
  });
});
