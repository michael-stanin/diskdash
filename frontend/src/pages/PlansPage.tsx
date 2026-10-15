import { useState } from "react";
import { CheckSquare, ChevronDown, Copy, Download, FolderOpen, ListChecks, Pencil, Plus, RefreshCw, Trash2, X } from "lucide-react";
import { Link } from "react-router";
import { PageHeader } from "@/app/PageHeader";
import { ApiError } from "@/api/client";
import {
  useCreatePlan,
  useDeletePlan,
  usePatchPlanItem,
  usePlan,
  usePlans,
  useRemovePlanItem,
  useRenamePlan,
  useReveal,
  useVerifyPlan,
} from "@/api/queries";
import type { PlanItemView } from "@/api/types/PlanItemView";
import type { PlanView } from "@/api/types/PlanView";
import { RiskBadge } from "@/components/Badges";
import { Instructions, useCopy } from "@/components/cleanup/CandidateItem";
import { protectionText } from "@/components/plans/AddToPlan";
import { RelativeTime, SizeDisplay } from "@/components/SizeDisplay";
import { StatCard, StatCardSkeleton } from "@/components/StatCard";
import { EmptyState, ErrorState } from "@/components/States";
import { useToast } from "@/components/Toast";
import { Button, buttonVariants } from "@/components/ui/button";
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";
import { formatBytes, formatCount } from "@/lib/format";
import { riskKey } from "@/lib/rule-categories";

const KIND_LABEL: Record<string, string> = {
  "manual-cleanup": "Remove by hand",
  "system-setting": "Windows setting",
  duplicate: "Duplicate copy",
  stale: "Old data",
  "manual-move": "Move to another drive",
};

const STATUS: { value: string; label: string }[] = [
  { value: "todo", label: "To do" },
  { value: "done", label: "Done" },
  { value: "skipped", label: "Skipped" },
];

function verifyText(i: PlanItemView): { text: string; tone: "good" | "warn" | "muted" } {
  const freed = i.verify.freed ?? 0;
  switch (i.verify.status) {
    case "removed":
      return { text: `Removed · ${formatBytes(freed)} freed`, tone: "good" };
    case "moved":
      return { text: `Moved · ${formatBytes(freed)} freed from here`, tone: "good" };
    case "partial":
      return { text: `Partly removed · ${formatBytes(freed)} freed`, tone: "warn" };
    case "present":
      return { text: "Still present · 0 B freed", tone: "warn" };
    case "grown":
      return { text: `Grew by ${formatBytes(Math.abs(freed))} since the plan`, tone: "warn" };
    default:
      return { text: "Not checked yet", tone: "muted" };
  }
}

function ItemRow({ plan, item }: { plan: number; item: PlanItemView }) {
  const [open, setOpen] = useState(false);
  // While typing, the draft wins; otherwise the saved note is shown.
  const [draft, setDraft] = useState<string | null>(null);
  const note = draft ?? item.note ?? "";
  const patch = usePatchPlanItem();
  const remove = useRemovePlanItem();
  const reveal = useReveal();
  const copy = useCopy();
  const { toast } = useToast();
  const v = verifyText(item);
  const blocked = item.blocked !== null;
  return (
    <li className={cn("flex flex-col gap-3 px-5 py-4", blocked && "opacity-60")} aria-label={item.title}>
      <div className="flex flex-wrap items-start gap-3">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
            <h3 className={cn("m-0 text-sm font-semibold", item.status === "skipped" && "line-through")}>{item.title}</h3>
            <RiskBadge risk={riskKey(item.risk as "Safe" | "Review" | "Expert")} />
            <span className="text-xs text-subtle-foreground">{KIND_LABEL[item.kind] ?? item.kind}</span>
            {blocked && (
              <span className="rounded-md bg-risk-expert-bg px-2 py-0.5 text-[11px] font-semibold text-risk-expert-fg" title={protectionText(item.blocked!)}>
                Now excluded
              </span>
            )}
          </div>
          <p className="m-0 min-w-0 truncate font-mono text-xs text-foreground-secondary" title={item.path}>
            {item.path}
          </p>
          {item.destination && <p className="m-0 text-xs text-muted-foreground">Planned destination: <span className="font-mono">{item.destination}</span></p>}
          <p className={cn("m-0 text-xs", v.tone === "good" ? "text-fill-normal-text" : v.tone === "warn" ? "text-fill-caution-text" : "text-subtle-foreground")}>
            {v.text}
            {item.verify.verified_at && (
              <span className="ml-1 text-subtle-foreground">
                (<RelativeTime date={item.verify.verified_at} />)
              </span>
            )}
          </p>
        </div>
        <span className="num text-base font-semibold" aria-label="Size on disk">
          {item.allocated === null ? <span className="text-sm font-medium text-muted-foreground">Size unknown</span> : <SizeDisplay bytes={item.allocated} />}
        </span>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <div role="radiogroup" aria-label={`Status of ${item.title}`} className="flex rounded-lg border border-border-strong p-0.5">
          {STATUS.map((s) => (
            <button
              key={s.value}
              type="button"
              role="radio"
              aria-checked={item.status === s.value}
              disabled={patch.isPending}
              onClick={() => patch.mutate({ plan, item: item.id, status: s.value })}
              className={cn(
                "h-7 cursor-pointer rounded-md px-2.5 text-xs font-medium transition-colors",
                item.status === s.value ? "bg-primary-soft text-primary-soft-foreground" : "text-muted-foreground hover:text-foreground",
              )}
            >
              {s.label}
            </button>
          ))}
        </div>
        <Input
          aria-label={`Note for ${item.title}`}
          placeholder="Add a note"
          className="h-8 min-w-40 flex-1 text-xs"
          value={note}
          maxLength={500}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => {
            if (note !== (item.note ?? "")) patch.mutate({ plan, item: item.id, note });
            setDraft(null);
          }}
        />
        {item.entry_id !== null && item.snapshot_id !== null && (
          <Button
            size="icon-sm"
            variant="outline"
            aria-label={`Open ${item.title} in Explorer`}
            title="Open in Explorer"
            onClick={() =>
              reveal.mutate(
                { snapshotId: item.snapshot_id!, entryId: item.entry_id! },
                { onError: (e) => toast(e instanceof ApiError && e.status === 410 ? "That item no longer exists. Rescan to refresh." : e.message, "error") },
              )
            }
          >
            <FolderOpen />
          </Button>
        )}
        <Button size="icon-sm" variant="outline" aria-label={`Copy path of ${item.title}`} title="Copy path" onClick={() => copy(item.path, "Path copied")}>
          <Copy />
        </Button>
        <Button size="sm" variant="secondary" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
          <ListChecks /> Steps <ChevronDown className={cn("transition-transform", open && "rotate-180")} />
        </Button>
        <Button size="icon-sm" variant="ghost" aria-label={`Remove ${item.title} from the plan`} title="Remove from plan" onClick={() => remove.mutate({ plan, item: item.id })}>
          <X />
        </Button>
      </div>
      {open && (
        <div className="rounded-xl border border-border bg-muted/40 p-4">
          <Instructions lines={item.instructions} />
        </div>
      )}
    </li>
  );
}

function Totals({ view }: { view: PlanView }) {
  const t = view.plan.totals;
  return (
    <section aria-label="Plan totals" className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
      <StatCard label="Items" value={formatCount(t.overall.count)} detail={t.unknown_size > 0 ? `${t.unknown_size} without a known size` : undefined} />
      <StatCard
        label="Space on disk"
        value={<SizeDisplay bytes={t.overall.allocated} variant="large" />}
        detail={
          <span>
            <SizeDisplay bytes={t.overall.logical} /> before rounding to clusters
          </span>
        }
      />
      <StatCard
        label="Done"
        value={<SizeDisplay bytes={t.done.allocated} variant="large" />}
        detail={`${t.done.count} done · ${t.todo.count} to do · ${t.skipped.count} skipped`}
      />
      <StatCard
        label="Freed so far"
        value={<SizeDisplay bytes={Math.max(0, view.freed_total)} variant="large" />}
        detail={view.verifications.length > 0 ? "Confirmed by a later scan" : "After you rescan, this shows what was really freed"}
      />
    </section>
  );
}

function PlanDetail({ id, onDeleted }: { id: number; onDeleted: () => void }) {
  const plan = usePlan(id);
  const rename = useRenamePlan();
  const del = useDeletePlan();
  const verify = useVerifyPlan();
  const { toast } = useToast();
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState("");
  const [confirm, setConfirm] = useState(false);

  if (plan.isError) {
    return (
      <Card>
        <ErrorState title="Cannot load this plan" error={plan.error} onRetry={() => plan.refetch()} />
      </Card>
    );
  }
  if (!plan.data) {
    return (
      <div className="flex flex-col gap-4" aria-label="Loading plan">
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
          {Array.from({ length: 4 }, (_, i) => (
            <StatCardSkeleton key={i} />
          ))}
        </div>
        <Skeleton className="h-48 w-full rounded-card" />
      </div>
    );
  }
  const view = plan.data;
  const items = view.items;
  return (
    <div className="flex flex-col gap-4">
      <Card className="flex flex-wrap items-center gap-3 px-5 py-4">
        {editing ? (
          <form
            className="flex min-w-0 flex-1 items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              rename.mutate({ id, name }, { onSuccess: () => setEditing(false), onError: (er) => toast(er.message, "error") });
            }}
          >
            <Input aria-label="Plan name" autoFocus value={name} maxLength={80} onChange={(e) => setName(e.target.value)} />
            <Button type="submit" variant="primary" size="sm">
              Save
            </Button>
            <Button type="button" variant="ghost" size="sm" onClick={() => setEditing(false)}>
              Cancel
            </Button>
          </form>
        ) : (
          <h2 className="m-0 min-w-0 flex-1 truncate text-lg font-semibold">{view.plan.name}</h2>
        )}
        {!editing && (
          <Button
            size="icon-sm"
            variant="ghost"
            aria-label="Rename the plan"
            onClick={() => {
              setName(view.plan.name);
              setEditing(true);
            }}
          >
            <Pencil />
          </Button>
        )}
        <a href={`/api/v1/plans/${id}/export?format=csv`} download className={cn(buttonVariants({ variant: "outline", size: "sm" }), "no-underline")} aria-label="Export the plan as CSV">
          <Download /> CSV
        </a>
        <a href={`/api/v1/plans/${id}/export?format=txt`} download className={cn(buttonVariants({ variant: "outline", size: "sm" }), "no-underline")} aria-label="Export the plan as a text checklist">
          <Download /> TXT
        </a>
        <Button
          size="sm"
          variant="secondary"
          disabled={verify.isPending || items.length === 0}
          onClick={() => verify.mutate(id, { onSuccess: () => toast("Checking against the latest scans"), onError: (e) => toast(e.message, "error") })}
        >
          <RefreshCw className={verify.isPending ? "animate-spin" : undefined} /> Check against latest scan
        </Button>
        <Button size="icon-sm" variant="ghost" aria-label="Delete the plan" title="Delete the plan (only the list, never your files)" onClick={() => setConfirm(true)}>
          <Trash2 />
        </Button>
      </Card>

      <Totals view={view} />

      {view.plan.totals.by_drive.length > 0 && (
        <p className="m-0 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground" aria-label="Per drive">
          {view.plan.totals.by_drive.map((d) => (
            <span key={d.drive}>
              {d.drive} {d.count} item{d.count === 1 ? "" : "s"}, <SizeDisplay bytes={d.allocated} />
            </span>
          ))}
        </p>
      )}
      {view.baseline_as_of && (
        <p className="m-0 text-xs text-muted-foreground">
          Sizes are from the scan of <RelativeTime date={view.baseline_as_of} />. diskdash never changes anything: after you have done the steps, rescan the drive and it checks what was really freed.
        </p>
      )}
      {view.verifications.length > 0 && (
        <Card className="px-5 py-3" aria-label="Verification">
          <ul className="m-0 flex list-none flex-col gap-1 p-0 text-[13px]">
            {view.verifications.map((v) => (
              <li key={v.root} className="flex flex-wrap items-baseline gap-x-3">
                <span className="font-mono text-xs">{v.root}</span>
                <span>
                  <SizeDisplay bytes={Math.max(0, v.freed)} className="font-semibold" /> freed by {v.items} planned item{v.items === 1 ? "" : "s"}
                </span>
                {v.scanned_change !== null && (
                  <span className="text-xs text-muted-foreground">
                    Scanned size changed by {v.scanned_change >= 0 ? "-" : "+"}
                    {formatBytes(Math.abs(v.scanned_change))} (includes unrelated activity)
                  </span>
                )}
                <span className="text-xs text-subtle-foreground">
                  checked <RelativeTime date={v.verified_at} />
                </span>
              </li>
            ))}
          </ul>
        </Card>
      )}

      <Card className="overflow-hidden">
        {items.length === 0 ? (
          <EmptyState
            icon={<ListChecks />}
            title="This plan is empty"
            description="Add suggestions from Recommendations, Duplicates or a drive's Stale tab. They become a checklist with the steps to do them yourself."
            action={
              <Button asChild variant="secondary">
                <Link to="/recommendations">Open Recommendations</Link>
              </Button>
            }
          />
        ) : (
          <ul className="m-0 list-none divide-y divide-border p-0" aria-label="Plan items">
            {items.map((i) => (
              <ItemRow key={i.id} plan={id} item={i} />
            ))}
          </ul>
        )}
      </Card>

      <Dialog open={confirm} onOpenChange={setConfirm}>
        <DialogContent
          title="Delete this plan?"
          description="This only removes the list from diskdash. Your files are not touched."
          footer={
            <>
              <Button variant="ghost" onClick={() => setConfirm(false)}>
                Cancel
              </Button>
              <Button
                variant="danger"
                onClick={() =>
                  del.mutate(id, {
                    onSuccess: () => {
                      setConfirm(false);
                      onDeleted();
                    },
                  })
                }
              >
                Delete plan
              </Button>
            </>
          }
        >
          <p className="m-0 px-6 py-4 text-[13px]">“{view.plan.name}” and its {items.length} item{items.length === 1 ? "" : "s"} will be removed from the list.</p>
        </DialogContent>
      </Dialog>
    </div>
  );
}

export function PlansPage() {
  const plans = usePlans();
  const create = useCreatePlan();
  const [selected, setSelected] = useState<number | null>(null);
  const [name, setName] = useState("");
  const list = plans.data ?? [];
  const current = selected !== null && list.some((p) => p.id === selected) ? selected : (list[0]?.id ?? null);

  return (
    <>
      <PageHeader title="Plans" subtitle="Checklists of suggestions you decide to carry out yourself. diskdash shows the steps and later checks what was freed; it never does them." />
      {plans.isError ? (
        <Card>
          <ErrorState title="Cannot load your plans" error={plans.error} onRetry={() => plans.refetch()} />
        </Card>
      ) : plans.isPending ? (
        <Card className="flex flex-col gap-3 p-5" aria-label="Loading plans">
          <Skeleton className="h-8 w-1/3" />
          <Skeleton className="h-24 w-full" />
        </Card>
      ) : list.length === 0 ? (
        <Card>
          <EmptyState
            icon={<CheckSquare />}
            title="No plans yet"
            description="A plan collects what you decide to clean up, with the exact steps for each item. Add suggestions from Recommendations to start one."
            action={
              <div className="flex flex-wrap items-center justify-center gap-2">
                <Button asChild variant="primary">
                  <Link to="/recommendations">Open Recommendations</Link>
                </Button>
                <form
                  className="flex items-center gap-2"
                  onSubmit={(e) => {
                    e.preventDefault();
                    create.mutate(name.trim() || "My cleanup plan", { onSuccess: (p) => setSelected(p.id) });
                  }}
                >
                  <Input aria-label="Name of a new plan" placeholder="Or start an empty plan" value={name} onChange={(e) => setName(e.target.value)} className="w-52" />
                  <Button type="submit" variant="secondary">
                    <Plus /> Create
                  </Button>
                </form>
              </div>
            }
          />
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-[260px_minmax(0,1fr)]">
          <Card className="h-fit overflow-hidden">
            <CardHeader>
              <CardTitle>Your plans</CardTitle>
            </CardHeader>
            <ul className="m-0 list-none divide-y divide-border p-0" aria-label="Plans">
              {list.map((p) => (
                <li key={p.id}>
                  <button
                    type="button"
                    aria-current={current === p.id}
                    onClick={() => setSelected(p.id)}
                    className={cn("flex w-full cursor-pointer flex-col gap-0.5 px-5 py-3 text-left hover:bg-hover/50", current === p.id && "bg-primary-soft/50")}
                  >
                    <span className="truncate text-[13px] font-semibold">{p.name}</span>
                    <span className="num text-xs text-muted-foreground">
                      {p.totals.overall.count} item{p.totals.overall.count === 1 ? "" : "s"} · {formatBytes(p.totals.overall.allocated)}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
            <form
              className="flex items-center gap-2 border-t border-border p-3"
              onSubmit={(e) => {
                e.preventDefault();
                create.mutate(name.trim() || "My cleanup plan", { onSuccess: (p) => { setSelected(p.id); setName(""); } });
              }}
            >
              <Input aria-label="Name of a new plan" placeholder="New plan" value={name} onChange={(e) => setName(e.target.value)} className="h-8 text-xs" maxLength={80} />
              <Button type="submit" size="icon-sm" variant="secondary" aria-label="Create plan">
                <Plus />
              </Button>
            </form>
          </Card>
          {current !== null && <PlanDetail key={current} id={current} onDeleted={() => setSelected(null)} />}
        </div>
      )}
    </>
  );
}
