import { act, render, screen } from "@testing-library/react";
import { sizeOf, SizeModeProvider, SizeModeToggle, useSizeMode } from "./size-mode";

function Probe() {
  const { measure } = useSizeMode();
  return <span data-testid="m">{measure}</span>;
}

describe("size mode", () => {
  beforeEach(() => localStorage.clear());

  it("defaults to size on disk and persists the choice", () => {
    const { unmount } = render(
      <SizeModeProvider>
        <SizeModeToggle />
        <Probe />
      </SizeModeProvider>,
    );
    expect(screen.getByTestId("m")).toHaveTextContent("alloc");
    act(() => screen.getByRole("button", { name: "Logical size" }).click());
    expect(screen.getByTestId("m")).toHaveTextContent("size");
    expect(screen.getByRole("button", { name: "Logical size" })).toHaveAttribute("aria-pressed", "true");
    unmount();

    render(
      <SizeModeProvider>
        <Probe />
      </SizeModeProvider>,
    );
    expect(screen.getByTestId("m")).toHaveTextContent("size");
  });

  it("picks the selected measure", () => {
    expect(sizeOf({ size: 1, alloc: 4096 }, "alloc")).toBe(4096);
    expect(sizeOf({ size: 1, alloc: 4096 }, "size")).toBe(1);
  });
});
