import { useMemo, useState } from "react";
import { AlertTriangle, ChevronDown, Copy, FileCode2, ListChecks, Search } from "lucide-react";
import { PageHeader } from "@/app/PageHeader";
import { useRerunRules, useRules } from "@/api/queries";
import type { Conditions } from "@/api/types/Conditions";
import type { RuleIssue } from "@/api/types/RuleIssue";
import type { RuleView } from "@/api/types/RuleView";
import { RiskBadge } from "@/components/Badges";
import { Instructions, useCopy } from "@/components/cleanup/CandidateItem";
import { EmptyState, ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card, CardBody, CardHeader, CardTitle } from "@/components/ui/card";
import { Select } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";
import { formatBytes } from "@/lib/format";
import { RULE_CATEGORIES, riskKey, ruleCategoryColor, ruleCategoryLabel } from "@/lib/rule-categories";

const EXAMPLE = `# %LOCALAPPDATA%\\diskdash\\rules\\mine.yaml
disabled: [windows-prefetch]      # turn a built-in rule off
rules:
  - id: my-render-cache
    category: apps
    title: Render cache
    explanation: Frames my renderer can recreate.
    risk: Safe
    kind: manual-cleanup
    paths: ['D:\\Render\\cache']
    instructions: Delete the folder in Explorer.
`;

function issueText(i: RuleIssue): string {
  const where = i.line !== null ? `:${i.line}${i.column !== null ? `:${i.column}` : ""}` : "";
  const who = i.rule_id ? ` · rule ${i.rule_id}` : i.index !== null ? ` · rule #${i.index}` : "";
  return `${i.file}${where}${who}`;
}

function conditionLines(c: Conditions, minAge: number | null): string[] {
  const names = (v: Conditions["sibling_exists"]) => (v ? (typeof v === "string" ? [v] : v).join(" or ") : "");
  const out: string[] = [];
  if (c.sibling_exists) out.push(`a file named ${names(c.sibling_exists)} sits next to it`);
  if (c.child_exists) out.push(`it contains ${names(c.child_exists)}`);
  if (c.min_size) out.push(`it takes at least ${formatBytes(c.min_size)} on disk`);
  if (minAge) out.push(`nothing in it changed for ${minAge} days`);
  if (c.extracted_sibling) out.push("a folder with the archive's name next to it holds at least half its size");
  return out;
}

function SourceBadge({ rule }: { rule: RuleView }) {
  const base = "inline-flex items-center rounded-md px-2 py-0.5 text-[11px] font-semibold";
  return (
    <>
      {rule.disabled && <span className={cn(base, "bg-muted text-muted-foreground")}>Disabled</span>}
      {rule.overrides_builtin && <span className={cn(base, "bg-primary-soft text-primary-soft-foreground")}>Overridden by you</span>}
      {rule.source.type === "user" && !rule.overrides_builtin && (
        <span className={cn(base, "bg-primary-soft text-primary-soft-foreground")}>Yours · {rule.source.file}</span>
      )}
      {rule.source.type === "builtin" && <span className="text-xs text-subtle-foreground">Built-in</span>}
    </>
  );
}

function RuleRow({ rule }: { rule: RuleView }) {
  const [open, setOpen] = useState(false);
  const conds = conditionLines(rule.conditions, rule.min_age_days);
  const panel = `rule-${rule.id}`;
  return (
    <li className={cn("flex flex-col gap-3 px-5 py-3.5", rule.disabled && "opacity-70")} aria-label={rule.title}>
      <div className="flex items-start gap-3">
        <span className="mt-1.5 size-2.5 shrink-0 rounded-full" style={{ background: ruleCategoryColor(rule.category) }} />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
            <h3 className="m-0 text-sm font-semibold">{rule.title}</h3>
            <RiskBadge risk={riskKey(rule.risk)} />
            <span className="text-xs text-subtle-foreground">{rule.kind === "system-setting" ? "Windows setting" : "Remove by hand"}</span>
            <SourceBadge rule={rule} />
          </div>
          <p className="m-0 text-[13px] leading-relaxed text-muted-foreground">{rule.explanation}</p>
        </div>
        <Button size="sm" variant="ghost" aria-expanded={open} aria-controls={panel} onClick={() => setOpen((o) => !o)}>
          Details <ChevronDown className={cn("transition-transform", open && "rotate-180")} />
        </Button>
      </div>
      {open && (
        <div id={panel} className="ml-[22px] flex flex-col gap-4 rounded-xl border border-border bg-muted/40 p-4 text-[13px]">
          <p className="m-0 font-mono text-xs text-subtle-foreground">id: {rule.id}</p>
          {(rule.expanded_paths.length > 0 || rule.globs.length > 0 || rule.virtual_source) && (
            <div className="flex flex-col gap-1.5">
              <h4 className="m-0 text-xs font-semibold text-muted-foreground">Where it looks</h4>
              {rule.virtual_source && <p className="m-0">Measured by Windows: {rule.virtual_source === "recycle-bin" ? "the Recycle Bin of each drive" : "shadow copy storage"}</p>}
              {rule.expanded_paths.map((p) => (
                <p key={p.template} className="m-0 font-mono text-xs break-all">
                  {p.path ?? <span className="text-warning">{p.template} — {p.error}</span>}
                </p>
              ))}
              {rule.globs.length > 0 && (
                <p className="m-0 text-xs text-muted-foreground">
                  {rule.expanded_paths.length > 0 ? "Inside it, matching " : "Anywhere on the drive, matching "}
                  <span className="font-mono">{rule.globs.join("  ")}</span>
                  {rule.applies_to && ` (${rule.applies_to === "dir" ? "folders" : "files"} only)`}
                </p>
              )}
            </div>
          )}
          {conds.length > 0 && (
            <div className="flex flex-col gap-1.5">
              <h4 className="m-0 text-xs font-semibold text-muted-foreground">Only suggested when</h4>
              <ul className="m-0 list-disc pl-5 text-muted-foreground">
                {conds.map((c) => (
                  <li key={c}>{c}</li>
                ))}
              </ul>
            </div>
          )}
          <div className="flex flex-col gap-1.5">
            <h4 className="m-0 text-xs font-semibold text-muted-foreground">What you do</h4>
            <Instructions lines={rule.instructions} />
          </div>
        </div>
      )}
    </li>
  );
}

export function RulesPage() {
  const rules = useRules();
  const rerun = useRerunRules();
  const copy = useCopy();
  const [text, setText] = useState("");
  const [category, setCategory] = useState("");
  const data = rules.data;

  const shown = useMemo(() => {
    const t = text.trim().toLowerCase();
    return (data?.rules ?? []).filter(
      (r) =>
        (!category || r.category === category) &&
        (!t || r.title.toLowerCase().includes(t) || r.id.includes(t) || r.explanation.toLowerCase().includes(t)),
    );
  }, [data, text, category]);
  const groups = useMemo(() => {
    const order = Object.keys(RULE_CATEGORIES);
    const map = new Map<string, RuleView[]>();
    for (const r of shown) map.set(r.category, [...(map.get(r.category) ?? []), r]);
    return [...map.entries()].sort((a, b) => order.indexOf(a[0]) - order.indexOf(b[0]));
  }, [shown]);
  const userRules = (data?.rules ?? []).filter((r) => r.source.type === "user" || r.disabled);

  return (
    <>
      <PageHeader
        title="Cleanup rules"
        breadcrumb={[{ label: "Recommendations", to: "/recommendations" }, { label: "Rules" }]}
        subtitle="The rules behind the suggestions. They are plain text files; diskdash never runs anything they describe."
        actions={
          <Button variant="secondary" disabled={rerun.isPending} onClick={() => rerun.mutate([])}>
            Re-run rules
          </Button>
        }
      />

      {rules.isError ? (
        <Card>
          <ErrorState title="Cannot load the rules" error={rules.error} onRetry={() => rules.refetch()} />
        </Card>
      ) : (
        <>
          {data && data.issues.length > 0 && (
            <Card className="border-danger/40" role="alert" aria-label="Rule problems">
              <CardHeader>
                <AlertTriangle className="size-4 text-danger" />
                <CardTitle>
                  {data.issues.length} rule problem{data.issues.length === 1 ? "" : "s"}: skipped, the other rules still work
                </CardTitle>
              </CardHeader>
              <ul className="m-0 flex list-none flex-col divide-y divide-border p-0">
                {data.issues.map((i, n) => (
                  <li key={n} className="flex flex-col gap-0.5 px-5 py-3 text-[13px]">
                    <span className="font-mono text-xs text-foreground-secondary">{issueText(i)}</span>
                    <span className="text-muted-foreground">{i.message}</span>
                  </li>
                ))}
              </ul>
            </Card>
          )}

          <Card>
            <CardHeader>
              <FileCode2 className="size-4 text-muted-foreground" />
              <CardTitle>Your own rules</CardTitle>
            </CardHeader>
            <CardBody className="flex flex-col gap-3 text-[13px]">
              {!data ? (
                <Skeleton className="h-10 w-full" />
              ) : (
                <>
                  <div className="flex flex-wrap items-center gap-2">
                    <code className="rounded-md bg-muted px-2.5 py-1.5 font-mono text-xs break-all">{data.user_dir.path}</code>
                    <Button size="sm" variant="outline" onClick={() => copy(data.user_dir.path, "Folder path copied")}>
                      <Copy /> Copy path
                    </Button>
                  </div>
                  <p className="m-0 text-muted-foreground">
                    Put <code className="font-mono">.yaml</code> files in this folder to add rules or change built-in ones. A rule with the same{" "}
                    <code className="font-mono">id</code> replaces the built-in one, and ids under <code className="font-mono">disabled:</code> are turned off. Then choose Re-run rules.
                  </p>
                  {userRules.length === 0 && (
                    <div className="flex flex-col gap-2">
                      <p className="m-0 font-medium">You have no rules of your own yet. An example:</p>
                      <div className="flex items-start gap-2">
                        <pre className="m-0 min-w-0 flex-1 overflow-x-auto rounded-md bg-muted px-3 py-2 font-mono text-xs">{EXAMPLE}</pre>
                        <Button size="icon-sm" variant="outline" aria-label="Copy the example" onClick={() => copy(EXAMPLE, "Example copied")}>
                          <Copy />
                        </Button>
                      </div>
                    </div>
                  )}
                </>
              )}
            </CardBody>
          </Card>

          <Card className="overflow-hidden">
            <div className="flex flex-wrap items-center gap-3 border-b border-border px-5 py-3">
              <label className="flex h-8 w-60 items-center gap-2 rounded-lg border border-border-strong px-2.5 text-muted-foreground focus-within:border-primary">
                <Search className="size-3.5 shrink-0" />
                <input
                  type="search"
                  value={text}
                  onChange={(e) => setText(e.target.value)}
                  placeholder="Search rules"
                  aria-label="Search rules"
                  className="w-full bg-transparent text-xs text-foreground outline-none placeholder:text-subtle-foreground"
                />
              </label>
              <Select aria-label="Category" className="h-8 w-auto text-xs" value={category} onChange={(e) => setCategory(e.target.value)}>
                <option value="">All categories</option>
                {Object.entries(RULE_CATEGORIES).map(([key, c]) => (
                  <option key={key} value={key}>
                    {c.label}
                  </option>
                ))}
              </Select>
              {data && (
                <span className="num ml-auto text-xs text-muted-foreground">
                  {shown.length} of {data.rules.length} rules
                </span>
              )}
            </div>
            {!data ? (
              <div className="flex flex-col gap-4 p-5" aria-label="Loading rules">
                {Array.from({ length: 5 }, (_, i) => (
                  <Skeleton key={i} className="h-12 w-full" />
                ))}
              </div>
            ) : shown.length === 0 ? (
              <EmptyState icon={<ListChecks />} title="No rules match" description="Try a different search or category." />
            ) : (
              groups.map(([key, list]) => (
                <section key={key} aria-label={ruleCategoryLabel(key)}>
                  <h2 className="m-0 border-b border-border bg-muted/50 px-5 py-2 text-xs font-semibold tracking-wide text-muted-foreground uppercase">
                    {ruleCategoryLabel(key)}
                  </h2>
                  <ul className="m-0 list-none divide-y divide-border p-0">
                    {list.map((r) => (
                      <RuleRow key={r.id} rule={r} />
                    ))}
                  </ul>
                </section>
              ))
            )}
          </Card>
        </>
      )}
    </>
  );
}
