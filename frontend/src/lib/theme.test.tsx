import { act, render, screen } from "@testing-library/react";
import { readPreference, resolveTheme, THEME_STORAGE_KEY, ThemeProvider, useTheme } from "./theme";

function Probe() {
  const { preference, resolved, setPreference } = useTheme();
  return (
    <div>
      <span data-testid="pref">{preference}</span>
      <span data-testid="resolved">{resolved}</span>
      <button onClick={() => setPreference("dark")}>dark</button>
    </div>
  );
}

function mockSystemDark(dark: boolean) {
  const listeners: ((e: MediaQueryListEvent) => void)[] = [];
  window.matchMedia = vi.fn().mockImplementation(() => ({
    matches: dark,
    addEventListener: (_: string, fn: (e: MediaQueryListEvent) => void) => listeners.push(fn),
    removeEventListener: () => {},
  }));
  return (next: boolean) => listeners.forEach((fn) => fn({ matches: next } as MediaQueryListEvent));
}

describe("theme", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.classList.remove("dark");
  });

  it("resolves preferences", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
  });

  it("defaults to system and ignores junk in storage", () => {
    expect(readPreference()).toBe("system");
    localStorage.setItem(THEME_STORAGE_KEY, "purple");
    expect(readPreference()).toBe("system");
  });

  it("persists the choice and applies the dark class", () => {
    mockSystemDark(false);
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    expect(screen.getByTestId("resolved")).toHaveTextContent("light");
    act(() => screen.getByText("dark").click());
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    expect(document.documentElement).toHaveClass("dark");
  });

  it("follows the OS while on system", () => {
    const flip = mockSystemDark(false);
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    act(() => flip(true));
    expect(screen.getByTestId("resolved")).toHaveTextContent("dark");
    expect(document.documentElement).toHaveClass("dark");
  });
});
