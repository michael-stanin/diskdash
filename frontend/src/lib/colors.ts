/** Colour maths for contrast checks and readable labels on coloured tiles. */

export function parseHex(hex: string): [number, number, number] | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const n = Number.parseInt(m[1]!, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** Parse `#rrggbb` or `rgb(r, g, b)` (what getComputedStyle returns). */
export function parseColor(c: string): [number, number, number] | null {
  const hex = parseHex(c);
  if (hex) return hex;
  const m = /rgba?\(\s*(\d+)[,\s]+(\d+)[,\s]+(\d+)/i.exec(c);
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}

export function luminance([r, g, b]: [number, number, number]): number {
  const lin = (v: number) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

export function contrast(a: [number, number, number], b: [number, number, number]): number {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (l1 + 0.05) / (l2 + 0.05);
}

/** Pick whichever of two text colours reads better on `bg`. */
export function readableOn(bg: string, dark: string, light: string): string {
  const b = parseColor(bg);
  const d = parseColor(dark);
  const l = parseColor(light);
  if (!b || !d || !l) return dark;
  return contrast(b, d) >= contrast(b, l) ? dark : light;
}
