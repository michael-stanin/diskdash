import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { CATEGORY_KEYS } from "./chart-theme";
import { contrast, parseHex, readableOn } from "./colors";

// CSS imports are stubbed in tests (css: false), so read the file itself.
const tokensCss = readFileSync(resolve(__dirname, "../styles/tokens.css"), "utf8");

/** Read `--name: #hex;` declarations from one block of tokens.css. */
function block(selector: ":root" | ".dark"): Record<string, string> {
  const start = tokensCss.indexOf(`${selector} {`);
  const body = tokensCss.slice(start, tokensCss.indexOf("}", start));
  const out: Record<string, string> = {};
  for (const m of body.matchAll(/--([a-z0-9-]+):\s*(#[0-9a-f]{6})/gi)) out[m[1]!] = m[2]!;
  return out;
}

describe("category colours", () => {
  for (const theme of [":root", ".dark"] as const) {
    const t = block(theme);
    it(`are distinguishable from cards in ${theme} (WCAG non-text 3:1)`, () => {
      const card = parseHex(t.card!)!;
      for (const k of CATEGORY_KEYS) {
        const c = parseHex(t[`cat-${k}`]!)!;
        expect(contrast(c, card), `cat-${k} on card`).toBeGreaterThanOrEqual(3);
      }
    });
    it(`carry a readable label (4.5:1) in ${theme}`, () => {
      const dark = t["on-color-dark"]!;
      const light = t["on-color-light"]!;
      for (const k of CATEGORY_KEYS) {
        const bg = t[`cat-${k}`]!;
        const label = readableOn(bg, dark, light);
        expect(contrast(parseHex(bg)!, parseHex(label)!), `label on cat-${k}`).toBeGreaterThanOrEqual(4.5);
      }
    });
  }

  it("contrast maths", () => {
    expect(contrast([0, 0, 0], [255, 255, 255])).toBeCloseTo(21, 0);
    expect(readableOn("#ffffff", "#000000", "#ffffff")).toBe("#000000");
  });
});
