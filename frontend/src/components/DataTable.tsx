import { useMemo, useRef, useState, type ReactNode } from "react";
import * as Popover from "@radix-ui/react-popover";
import {
  flexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getSortedRowModel,
  useReactTable,
  type ColumnDef,
  type SortingState,
  type VisibilityState,
} from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp, Columns3, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/cn";

declare module "@tanstack/react-table" {
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  interface ColumnMeta<TData, TValue> {
    /** CSS grid track, e.g. "minmax(0,1fr)" or "96px". */
    width?: string;
    align?: "left" | "right";
    /** Use the monospace face (paths). */
    mono?: boolean;
  }
}

export interface DataTableProps<T> {
  columns: ColumnDef<T, unknown>[];
  data: T[];
  getRowId: (row: T) => string;
  rowActions?: (row: T) => ReactNode;
  initialSorting?: SortingState;
  filterPlaceholder?: string;
  /** Height of the scroll viewport in px. */
  height?: number;
  rowHeight?: number;
  emptyMessage?: string;
  ariaLabel: string;
}

const ROW_HEIGHT = 44;

/** Sortable, filterable, column-configurable table that virtualises rows. */
export function DataTable<T>({
  columns,
  data,
  getRowId,
  rowActions,
  initialSorting = [],
  filterPlaceholder = "Filter",
  height = 440,
  rowHeight = ROW_HEIGHT,
  emptyMessage = "No rows match the filter.",
  ariaLabel,
}: DataTableProps<T>) {
  const [sorting, setSorting] = useState<SortingState>(initialSorting);
  const [filter, setFilter] = useState("");
  const [visibility, setVisibility] = useState<VisibilityState>({});

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Table returns stable functions by design
  const table = useReactTable({
    data,
    columns,
    getRowId,
    state: { sorting, globalFilter: filter, columnVisibility: visibility },
    onSortingChange: setSorting,
    onGlobalFilterChange: setFilter,
    onColumnVisibilityChange: setVisibility,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
  });

  const visibleColumns = table.getVisibleLeafColumns();
  const template = useMemo(
    () =>
      [...visibleColumns.map((c) => c.columnDef.meta?.width ?? "minmax(0,1fr)"), rowActions ? "auto" : null]
        .filter(Boolean)
        .join(" "),
    [visibleColumns, rowActions],
  );

  const rows = table.getRowModel().rows;
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: 8,
    initialRect: { width: 800, height },
  });

  return (
    <div className="flex flex-col">
      <div className="flex items-center gap-2 border-b border-border px-4 py-3">
        <label className="flex h-8 w-56 items-center gap-2 rounded-lg border border-border-strong px-2.5 text-muted-foreground focus-within:border-primary">
          <Search className="size-3.5 shrink-0" />
          <input
            type="search"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder={filterPlaceholder}
            aria-label={filterPlaceholder}
            className="w-full bg-transparent text-xs text-foreground outline-none placeholder:text-subtle-foreground"
          />
        </label>
        <span className="num text-xs text-subtle-foreground">{rows.length.toLocaleString("en-US")} rows</span>
        <Popover.Root>
          <Popover.Trigger asChild>
            <Button variant="ghost" size="sm" className="ml-auto" aria-label="Choose columns">
              <Columns3 /> Columns
            </Button>
          </Popover.Trigger>
          <Popover.Portal>
            <Popover.Content
              align="end"
              sideOffset={6}
              className="z-50 flex min-w-44 flex-col gap-1 rounded-xl border border-border bg-card-raised p-2 shadow-popover"
            >
              {table.getAllLeafColumns().map((col) => (
                <label key={col.id} className="flex items-center gap-2 rounded-md px-2 py-1.5 text-[13px] hover:bg-hover">
                  <input
                    type="checkbox"
                    checked={col.getIsVisible()}
                    disabled={!col.getCanHide()}
                    onChange={col.getToggleVisibilityHandler()}
                    className="accent-primary"
                  />
                  {typeof col.columnDef.header === "string" ? col.columnDef.header : col.id}
                </label>
              ))}
            </Popover.Content>
          </Popover.Portal>
        </Popover.Root>
      </div>

      <div role="table" aria-label={ariaLabel} aria-rowcount={rows.length + 1} className="text-[13px]">
        <div role="rowgroup">
          {table.getHeaderGroups().map((hg) => (
            <div
              role="row"
              key={hg.id}
              className="grid items-center gap-4 border-b border-border px-4 py-2 text-xs text-muted-foreground"
              style={{ gridTemplateColumns: template }}
            >
              {hg.headers.map((h) => {
                const sorted = h.column.getIsSorted();
                const align = h.column.columnDef.meta?.align;
                return (
                  <div
                    role="columnheader"
                    key={h.id}
                    aria-sort={sorted === "asc" ? "ascending" : sorted === "desc" ? "descending" : "none"}
                    className={cn(align === "right" && "text-right")}
                  >
                    {h.column.getCanSort() ? (
                      <button
                        type="button"
                        onClick={h.column.getToggleSortingHandler()}
                        className={cn(
                          "inline-flex cursor-pointer items-center gap-1 rounded hover:text-foreground",
                          align === "right" && "flex-row-reverse",
                        )}
                      >
                        {flexRender(h.column.columnDef.header, h.getContext())}
                        {sorted === "asc" && <ArrowUp className="size-3" />}
                        {sorted === "desc" && <ArrowDown className="size-3" />}
                      </button>
                    ) : (
                      flexRender(h.column.columnDef.header, h.getContext())
                    )}
                  </div>
                );
              })}
              {rowActions && <div role="columnheader" aria-label="Actions" />}
            </div>
          ))}
        </div>

        <div ref={scrollRef} role="rowgroup" className="overflow-auto" style={{ maxHeight: height }}>
          {rows.length === 0 ? (
            <div className="px-4 py-10 text-center text-[13px] text-muted-foreground">{emptyMessage}</div>
          ) : (
            <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
              {virtualizer.getVirtualItems().map((vi) => {
                const row = rows[vi.index]!;
                return (
                  <div
                    role="row"
                    aria-rowindex={vi.index + 2}
                    key={row.id}
                    className="absolute inset-x-0 grid items-center gap-4 border-b border-border/60 px-4 hover:bg-hover/50"
                    style={{ gridTemplateColumns: template, height: vi.size, transform: `translateY(${vi.start}px)` }}
                  >
                    {row.getVisibleCells().map((cell) => {
                      const meta = cell.column.columnDef.meta;
                      return (
                        <div
                          role="cell"
                          key={cell.id}
                          className={cn(
                            "min-w-0 truncate",
                            meta?.align === "right" && "num text-right",
                            meta?.mono && "font-mono text-xs text-foreground-secondary",
                          )}
                        >
                          {flexRender(cell.column.columnDef.cell, cell.getContext())}
                        </div>
                      );
                    })}
                    {rowActions && (
                      <div role="cell" className="flex justify-end gap-1">
                        {rowActions(row.original)}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
