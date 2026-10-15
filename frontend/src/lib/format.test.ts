import {
  formatBytes,
  formatBytesParts,
  formatDuration,
  formatExactBytes,
  formatPercent,
  formatRelative,
} from "./format";

describe("formatBytes", () => {
  it.each([
    [0, "0 B"],
    [512, "512 B"],
    [1023, "1023 B"],
    [1024, "1 KiB"],
    [1536, "1.5 KiB"],
    [10 * 1024 * 1024, "10 MiB"],
    [123456789012, "115 GiB"],
    [44023414784, "41 GiB"],
    [1.6 * 1024 ** 4, "1.6 TiB"],
    [33500744908, "31.2 GiB"],
    [1024 * 1024 - 1, "1 MiB"],
    [200 * 1024 ** 2, "200 MiB"],
    [100 * 1024 ** 3, "100 GiB"],
    [500 * 1024 ** 3, "500 GiB"],
  ])("%d → %s", (bytes, expected) => {
    expect(formatBytes(bytes)).toBe(expected);
  });

  it("never shows more than three significant figures", () => {
    for (let b = 1024; b < 1e15; b *= 3.7) {
      const digits = formatBytes(b).split(" ")[0]!.replace(".", "").replace(/^0+/, "");
      expect(digits.length).toBeLessThanOrEqual(3);
    }
  });

  it("handles invalid input", () => {
    expect(formatBytes(-1)).toBe("—");
    expect(formatBytes(Number.NaN)).toBe("—");
  });

  it("splits into value and unit", () => {
    expect(formatBytesParts(123456789012)).toEqual({ value: "115", unit: "GiB" });
  });
});

describe("other formatters", () => {
  it("exact bytes", () => {
    expect(formatExactBytes(123456789012)).toBe("123,456,789,012 bytes");
  });

  it("percent", () => {
    expect(formatPercent(0.086, 1)).toBe("8.6%");
    expect(formatPercent(0.91)).toBe("91%");
  });

  it("relative time", () => {
    const now = new Date("2026-10-12T12:00:00Z");
    expect(formatRelative("2026-10-10T12:00:00Z", now)).toBe("2 days ago");
    expect(formatRelative("2026-07-12T12:00:00Z", now)).toBe("3 months ago");
    expect(formatRelative("2026-10-12T11:59:50Z", now)).toBe("just now");
  });

  it("duration", () => {
    expect(formatDuration(372)).toBe("6 min 12 s");
    expect(formatDuration(45)).toBe("45 s");
    expect(formatDuration(3780)).toBe("1 h 3 min");
  });
});
