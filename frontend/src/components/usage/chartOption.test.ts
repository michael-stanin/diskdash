import type { EntryView } from "@/api/types/EntryView";
import type { TreeNode } from "@/api/types/TreeNode";
import { buildOption, toNodes, tooltipHtml, type Palette } from "./chartOption";

function entry(over: Partial<EntryView>): EntryView {
  return {
    id: 1,
    name: "x",
    path: "C:\\x",
    kind: "dir",
    size: 100,
    alloc: 120,
    modified: null,
    category: null,
    top_category: null,
    ext: null,
    file_count: 3,
    dir_count: 0,
    newest_modified: null,
    access_denied: false,
    cloud_placeholder: false,
    hardlink_repeat: false,
    link_target: null,
    ...over,
  };
}

const palette: Palette = { color: (c) => `color-${c ?? "none"}`, labelOn: () => "label" };

const tree: TreeNode = {
  entry: entry({ id: 0, name: "D:\\", size: 1000, alloc: 1100 }),
  children: [
    {
      entry: entry({ id: 2, name: "Videos", size: 600, alloc: 640, top_category: "video" }),
      children: [{ entry: entry({ id: 5, name: "a.mkv", kind: "file", size: 600, alloc: 640, category: "video" }), children: null, other: null }],
      other: null,
    },
    { entry: entry({ id: 3, name: "Locked", size: 0, alloc: 0, access_denied: true }), children: null, other: null },
    { entry: entry({ id: 4, name: "notes.txt", kind: "file", size: 50, alloc: 4096, category: "documents" }), children: null, other: null },
  ],
  other: { count: 12, size: 350, alloc: 380 },
};

describe("chart data", () => {
  it("maps folders by dominant category and files by own category", () => {
    const nodes = toNodes(tree, "size", palette);
    expect(nodes[0]).toMatchObject({ name: "Videos", value: 600, kind: "dir", itemStyle: { color: "color-video" } });
    expect(nodes[0]!.children![0]).toMatchObject({ name: "a.mkv", itemStyle: { color: "color-video" } });
    expect(nodes.find((n) => n.name === "notes.txt")!.itemStyle!.color).toBe("color-documents");
  });

  it("adds an explorable Other node and uses the selected size", () => {
    const nodes = toNodes(tree, "alloc", palette);
    const other = nodes.at(-1)!;
    expect(other).toMatchObject({ kind: "other", value: 380, parentId: 0, shown: 3, otherCount: 12 });
    expect(nodes[0]!.value).toBe(640);
  });

  it("drops zero-size tiles (ECharts cannot draw them)", () => {
    expect(toNodes(tree, "size", palette).some((n) => n.name === "Locked")).toBe(false);
  });

  it("tooltips explain incomplete folders and escape names", () => {
    expect(tooltipHtml({ name: "Locked", value: 10, kind: "dir", incomplete: true }, 100)).toContain("lower bound");
    expect(tooltipHtml({ name: "<b>x</b>", value: 1, kind: "file" }, 1)).not.toContain("<b>x</b>");
  });

  it("turns animation off for reduced motion", () => {
    const o = buildOption(tree, "size", "treemap", palette, { reducedMotion: true, gapColor: "g" });
    expect(o.animation).toBe(false);
    expect(o.animationDurationUpdate).toBe(0);
    const s = buildOption(tree, "size", "sunburst", palette, { reducedMotion: false, gapColor: "g" });
    expect((s.series as { type: string }[])[0]!.type).toBe("sunburst");
    expect(s.animationDurationUpdate).toBeLessThanOrEqual(200);
  });
});
