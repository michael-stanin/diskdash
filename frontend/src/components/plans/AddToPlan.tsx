import { useState } from "react";
import { ListPlus } from "lucide-react";
import { Link } from "react-router";
import { ApiError } from "@/api/client";
import { useAddToPlan, useCreatePlan, usePlans, type AddToPlanBody } from "@/api/queries";
import { useToast } from "@/components/Toast";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { formatBytes } from "@/lib/format";

const LAST_PLAN = "diskdash.lastPlan";

function remembered(): number | null {
  try {
    const v = Number(localStorage.getItem(LAST_PLAN));
    return Number.isInteger(v) && v > 0 ? v : null;
  } catch {
    return null;
  }
}

/** Plain-language text for a protection reason such as `protected:program_files`. */
export function protectionText(reason: string): string {
  const what: Record<string, string> = {
    "protected:windows": "the Windows folder",
    "protected:system32": "the Windows system folder",
    "protected:program_files": "Program Files",
    "protected:program_data": "the ProgramData folder",
    "protected:profile_root": "the user profiles folder",
    "protected:user_folder": "one of your personal folders",
    "protected:drive_root": "a drive root",
    "protected:system_file": "a Windows system file (use its Windows setting instead)",
    "protected:system_volume_information": "System Volume Information",
    "protected:recycle_bin": "the Recycle Bin folder",
    "protected:boot": "the boot files",
    "protected:recovery": "the recovery files",
    "protected:user": "a folder you marked as protected",
    "excluded:user": "a folder you excluded",
  };
  return `This is ${what[reason] ?? "a protected location"}, so it is never suggested or planned. (${reason})`;
}

interface Props {
  body: AddToPlanBody;
  label?: string;
  /** What is being added, for the dialog (path or title). */
  what: string;
  size?: "sm" | "icon-sm";
}

/** "Add to plan": pick or create a plan. Nothing is done to the item; it joins a checklist. */
export function AddToPlanButton({ body, what, label = "Add to plan", size = "sm" }: Props) {
  const [open, setOpen] = useState(false);
  // Plans are only fetched once the dialog is opened.
  const plans = usePlans(open);
  const create = useCreatePlan();
  const add = useAddToPlan();
  const { toast } = useToast();
  const [chosen, setChosen] = useState<number | null>(null);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const list = Array.isArray(plans.data) ? plans.data : [];
  const selected = chosen ?? remembered() ?? list[0]?.id ?? null;
  const creating = list.length === 0 || selected === -1;

  const run = async () => {
    setError(null);
    try {
      let plan = selected;
      let planName = list.find((p) => p.id === plan)?.name ?? "the plan";
      if (creating) {
        const made = await create.mutateAsync(name.trim() || "My cleanup plan");
        plan = made.id;
        planName = made.name;
      }
      if (plan === null || plan === -1) return;
      const r = await add.mutateAsync({ plan, body });
      try {
        localStorage.setItem(LAST_PLAN, String(plan));
      } catch {
        /* remembering the plan is a convenience */
      }
      toast(r.added ? `Added to ${planName}` : `Already in ${planName}`, r.added ? "success" : "info");
      setOpen(false);
    } catch (e) {
      if (e instanceof ApiError && e.status === 422 && e.fields.path) setError(protectionText(e.fields.path));
      else setError(e instanceof Error ? e.message : "Could not add it");
    }
  };

  return (
    <>
      <Button
        size={size}
        variant="outline"
        aria-label={`${label}: ${what}`}
        title={label}
        onClick={() => {
          setError(null);
          setOpen(true);
        }}
      >
        <ListPlus /> {size === "sm" && label}
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent
          title="Add to a cleanup plan"
          description={<span className="break-all">{what}</span>}
          footer={
            <>
              <Button variant="ghost" onClick={() => setOpen(false)}>
                Cancel
              </Button>
              <Button variant="primary" disabled={add.isPending || create.isPending} onClick={() => void run()}>
                Add
              </Button>
            </>
          }
        >
          <div className="flex flex-col gap-3 px-6 py-4 text-[13px]">
            <p className="m-0 text-muted-foreground">
              A plan is a checklist you carry out yourself. Adding something here does not change it in any way.
            </p>
            {plans.isPending ? (
              <Skeleton className="h-16 w-full" />
            ) : (
              <fieldset className="m-0 flex flex-col gap-2 border-0 p-0">
                <legend className="sr-only">Plan</legend>
                {list.map((p) => (
                  <label key={p.id} className="flex cursor-pointer items-center gap-2.5 rounded-lg border border-border px-3 py-2 hover:bg-hover/50">
                    <input type="radio" name="plan" className="accent-primary" checked={!creating && selected === p.id} onChange={() => setChosen(p.id)} />
                    <span className="min-w-0 flex-1 truncate font-medium">{p.name}</span>
                    <span className="num text-xs text-muted-foreground">
                      {p.totals.overall.count} item{p.totals.overall.count === 1 ? "" : "s"} · {formatBytes(p.totals.overall.allocated)}
                    </span>
                  </label>
                ))}
                {list.length > 0 && (
                  <label className="flex cursor-pointer items-center gap-2.5 rounded-lg border border-border px-3 py-2 hover:bg-hover/50">
                    <input type="radio" name="plan" className="accent-primary" checked={creating} onChange={() => setChosen(-1)} />
                    <span className="font-medium">A new plan</span>
                  </label>
                )}
                {creating && (
                  <Input aria-label="New plan name" placeholder="Name of the new plan" value={name} onChange={(e) => setName(e.target.value)} maxLength={80} />
                )}
              </fieldset>
            )}
            {error && (
              <p role="alert" className="m-0 text-danger">
                {error}
              </p>
            )}
            <Link to="/plans" className="text-xs text-primary no-underline hover:text-primary-hover" onClick={() => setOpen(false)}>
              Open Plans
            </Link>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}
