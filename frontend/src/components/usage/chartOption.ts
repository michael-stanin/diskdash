import type { Measure } from "@/api/types/Measure";
import type { OtherBucket } from "@/api/types/OtherBucket";
import type { TreeNode } from "@/api/types/TreeNode";
import { formatBytes, formatCount, formatPercent } from "@/lib/format";
import { sizeOf } from "@/lib/size-mode";

export type ChartView = "treemap" | "sunburst";

/** Extra data carried on every chart node (read back on click). */
export interface NodeData {
  name: string;
  value: number;
  id?: number;
  kind?: "dir" | "file" | "link" | "other";
  path?: string;
  incomplete?: boolean;
  fileCount?: number | null;
  /** For "Other": the folder whose remaining children it stands for, and how many are shown. */
  parentId?: number;
  shown?: number;
  otherCount?: number;
  itemStyle?: { color: string; borderColor?: string };
  label?: { color: string };
  upperLabel?: { color: string };
  children?: NodeData[];
}

export interface Palette {
  /** Colour of a category key (files: own category; folders: dominant one). */
  color: (category: string | null | undefined) => string;
  /** Readable label colour on a given fill. */
  labelOn: (fill: string) => string;
}

function otherNode(o: OtherBucket, parentId: number, shown: number, m: Measure, p: Palette): NodeData {
  const fill = p.color("other");
  return {
    name: `Other · ${formatCount(o.count)} more`,
    value: sizeOf(o, m),
    kind: "other",
    parentId,
    shown,
    otherCount: o.count,
    itemStyle: { color: fill },
    label: { color: p.labelOn(fill) },
  };
}

export function toNodes(n: TreeNode, m: Measure, p: Palette): NodeData[] {
  const kids = n.children ?? [];
  const out: NodeData[] = kids.map((k) => {
    const e = k.entry;
    const fill = p.color(e.kind === "dir" ? e.top_category : e.category);
    const node: NodeData = {
      name: e.name,
      value: sizeOf(e, m),
      id: e.id,
      kind: e.kind,
      path: e.path,
      incomplete: e.access_denied,
      fileCount: e.file_count,
      itemStyle: { color: fill, borderColor: fill },
      label: { color: p.labelOn(fill) },
      upperLabel: { color: p.labelOn(fill) },
    };
    if (k.children) node.children = toNodes(k, m, p);
    return node;
  });
  if (n.other) out.push(otherNode(n.other, n.entry.id, kids.length, m, p));
  // ECharts drops zero-value nodes in a treemap; keep the data honest elsewhere.
  return out.filter((d) => d.value > 0);
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

export function tooltipHtml(d: NodeData, total: number): string {
  const lines = [`<strong>${escapeHtml(d.name)}</strong>`, `${formatBytes(d.value)} · ${formatPercent(total > 0 ? d.value / total : 0, 1)}`];
  if (d.kind === "dir" && d.fileCount != null) lines.push(`${formatCount(d.fileCount)} files`);
  if (d.kind === "other") lines.push("Click to list them");
  else if (d.kind === "dir") lines.push(d.incomplete ? "Incomplete: access was denied, size is a lower bound" : "Click to open");
  return lines.join("<br/>");
}

export function buildOption(
  root: TreeNode,
  m: Measure,
  view: ChartView,
  p: Palette,
  opts: { reducedMotion: boolean; gapColor: string },
): Record<string, unknown> {
  const data = toNodes(root, m, p);
  const total = sizeOf(root.entry, m);
  const animation = !opts.reducedMotion;
  const tooltip = {
    trigger: "item",
    confine: true,
    formatter: (params: { data?: NodeData }) => (params.data ? tooltipHtml(params.data, total) : ""),
  };
  if (view === "sunburst") {
    return {
      animation,
      animationDurationUpdate: animation ? 180 : 0,
      tooltip,
      series: [
        {
          type: "sunburst",
          data,
          radius: ["14%", "96%"],
          nodeClick: false,
          sort: undefined,
          itemStyle: { borderColor: opts.gapColor, borderWidth: 2 },
          label: { rotate: "radial", minAngle: 10, overflow: "truncate", width: 90, fontSize: 11 },
          emphasis: { focus: "ancestor" },
        },
      ],
    };
  }
  return {
    animation,
    animationDurationUpdate: animation ? 180 : 0,
    tooltip,
    series: [
      {
        type: "treemap",
        data,
        roam: false,
        nodeClick: false,
        breadcrumb: { show: false },
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        squareRatio: 0.5 * (1 + Math.sqrt(5)),
        label: { show: true, formatter: "{b}", fontSize: 12, fontWeight: 600, overflow: "truncate" },
        // Header strips only on our first level (set in `levels`), never on the root.
        upperLabel: { show: false, height: 22, fontSize: 12, fontWeight: 600, overflow: "truncate" },
        // levels[0] is ECharts' invisible root; [1] our folders/files; [2] their preview children.
        levels: [
          { itemStyle: { gapWidth: 3, borderWidth: 0 }, upperLabel: { show: false } },
          // Folder header strips take the folder's own (category) colour.
          { itemStyle: { gapWidth: 1, borderWidth: 3, borderRadius: 6 }, upperLabel: { show: true } },
          { itemStyle: { gapWidth: 1, borderWidth: 0, borderRadius: 3 }, upperLabel: { show: false } },
        ],
      },
    ],
  };
}
