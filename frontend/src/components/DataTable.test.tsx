import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ColumnDef } from "@tanstack/react-table";
import { DataTable } from "./DataTable";

interface Row {
  id: number;
  path: string;
  size: number;
}

const columns: ColumnDef<Row, unknown>[] = [
  { accessorKey: "path", header: "Path", meta: { mono: true } },
  { accessorKey: "size", header: "Size", meta: { align: "right", width: "96px" } },
];

function rows(n: number): Row[] {
  return Array.from({ length: n }, (_, i) => ({ id: i, path: `D:\\folder\\item-${i}`, size: (i * 7919) % 100_000 }));
}

function bodyRows() {
  const groups = screen.getAllByRole("rowgroup");
  return within(groups[1]!).queryAllByRole("row");
}

// jsdom has no layout; give the scroll viewport a real size so the virtualiser renders rows.
beforeAll(() => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    width: 800, height: 440, top: 0, left: 0, right: 800, bottom: 440, x: 0, y: 0, toJSON: () => ({}),
  } as DOMRect);
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 440 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 800 });
});

describe("DataTable", () => {
  it("renders 100k rows quickly by virtualising", () => {
    const data = rows(100_000);
    const start = performance.now();
    render(<DataTable ariaLabel="Files" columns={columns} data={data} getRowId={(r) => String(r.id)} height={440} />);
    const elapsed = performance.now() - start;
    expect(screen.getByText("100,000 rows")).toBeInTheDocument();
    expect(bodyRows().length).toBeGreaterThan(5);
    expect(bodyRows().length).toBeLessThan(40);
    expect(elapsed).toBeLessThan(6_000); // generous: other test files run in parallel
  });

  it("sorts when a header is clicked", () => {
    render(<DataTable ariaLabel="Files" columns={columns} data={rows(50)} getRowId={(r) => String(r.id)} />);
    fireEvent.click(screen.getByRole("button", { name: /Size/ }));
    const header = screen.getByRole("columnheader", { name: /Size/ });
    // Numeric columns sort largest-first on the first click (what you want for sizes).
    expect(header).toHaveAttribute("aria-sort", "descending");
    const sizes = bodyRows()
      .slice(0, 10)
      .map((r) => Number(within(r).getAllByRole("cell")[1]!.textContent));
    expect(sizes).toEqual([...sizes].sort((a, b) => b - a));
    fireEvent.click(screen.getByRole("button", { name: /Size/ }));
    expect(header).toHaveAttribute("aria-sort", "ascending");
  });

  it("filters rows and shows the empty message", () => {
    render(<DataTable ariaLabel="Files" columns={columns} data={rows(50)} getRowId={(r) => String(r.id)} />);
    fireEvent.change(screen.getByLabelText("Filter"), { target: { value: "item-42" } });
    expect(screen.getByText("1 rows")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Filter"), { target: { value: "nothing-matches" } });
    expect(screen.getByText("No rows match the filter.")).toBeInTheDocument();
  });

  it("renders row actions", () => {
    render(
      <DataTable
        ariaLabel="Files"
        columns={columns}
        data={rows(3)}
        getRowId={(r) => String(r.id)}
        rowActions={(r) => <button>open {r.id}</button>}
      />,
    );
    expect(screen.getByText("open 0")).toBeInTheDocument();
  });
});
