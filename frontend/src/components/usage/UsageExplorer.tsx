import { useState } from "react";
import type { Snapshot } from "@/api/types/Snapshot";
import { Breakdown } from "./Breakdown";
import type { ChartView } from "./chartOption";
import { OtherDialog, type OtherTarget } from "./OtherDialog";
import { TopTable } from "./TopTables";
import { UsageChart } from "./UsageChart";

const VIEW_KEY = "diskdash.chartView";

function readView(): ChartView {
  try {
    return localStorage.getItem(VIEW_KEY) === "sunburst" ? "sunburst" : "treemap";
  } catch {
    return "treemap";
  }
}

/** Interactive analysis of one complete snapshot: chart, breakdown, top lists. */
export function UsageExplorer({ snapshot }: { snapshot: Snapshot }) {
  const [node, setNode] = useState(0);
  const [view, setViewState] = useState<ChartView>(readView);
  const [category, setCategory] = useState<string | null>(null);
  const [other, setOther] = useState<OtherTarget | null>(null);

  const setView = (v: ChartView) => {
    setViewState(v);
    try {
      localStorage.setItem(VIEW_KEY, v);
    } catch {
      // Not persisted.
    }
  };
  const goTo = (id: number) => {
    setNode(id);
    setCategory(null);
  };

  return (
    <div className="flex flex-col gap-4">
      <UsageChart
        snapshotId={snapshot.id}
        node={node}
        view={view}
        onView={setView}
        onNode={goTo}
        onOther={(parentId, shown, count) => setOther({ parentId, shown, count })}
      />
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-5">
        <div className="xl:col-span-2">
          <Breakdown snapshotId={snapshot.id} node={node} category={category} onCategory={setCategory} />
        </div>
        <div className="xl:col-span-3">
          <TopTable snapshotId={snapshot.id} node={node} kind="dirs" onShow={goTo} />
        </div>
      </div>
      <TopTable
        snapshotId={snapshot.id}
        node={node}
        kind="files"
        category={category}
        onClearCategory={() => setCategory(null)}
      />
      <OtherDialog snapshotId={snapshot.id} target={other} onClose={() => setOther(null)} onShow={goTo} />
    </div>
  );
}
