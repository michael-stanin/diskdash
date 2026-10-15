import { useState } from "react";
import { Plus, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

const FOLDER = /^[A-Za-z]:\\.*/;

interface Props {
  id: string;
  label: string;
  hint: string;
  value: string[];
  onChange: (next: string[]) => void;
  error?: string;
  emptyText: string;
}

/** An editable list of folder paths (exclusions, protected folders). */
export function PathList({ id, label, hint, value, onChange, error, emptyText }: Props) {
  const [text, setText] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const add = () => {
    const t = text.trim().replace(/\/+/g, "\\");
    if (!FOLDER.test(t)) {
      setProblem("Enter a full folder path, for example D:\\Photos");
      return;
    }
    if (value.some((v) => v.toLowerCase() === t.toLowerCase())) {
      setProblem("That folder is already in the list");
      return;
    }
    setProblem(null);
    setText("");
    onChange([...value, t]);
  };
  return (
    <div className="flex flex-col gap-2 py-3">
      <div className="flex flex-col gap-0.5">
        <label htmlFor={id} className="text-[13px] font-medium">
          {label}
        </label>
        <span className="text-xs text-muted-foreground">{hint}</span>
      </div>
      {value.length === 0 ? (
        <p className="m-0 rounded-lg border border-dashed border-border px-3 py-2.5 text-xs text-muted-foreground">{emptyText}</p>
      ) : (
        <ul className="m-0 flex list-none flex-col gap-1 p-0" aria-label={label}>
          {value.map((p) => (
            <li key={p} className="flex items-center gap-2 rounded-lg bg-muted px-3 py-1.5">
              <span className="min-w-0 flex-1 truncate font-mono text-xs" title={p}>
                {p}
              </span>
              <Button size="icon-sm" variant="ghost" aria-label={`Remove ${p}`} onClick={() => onChange(value.filter((v) => v !== p))}>
                <X />
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className="flex gap-2">
        <Input
          id={id}
          placeholder="D:\Photos"
          value={text}
          aria-invalid={Boolean(problem || error)}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add();
            }
          }}
        />
        <Button variant="secondary" onClick={add}>
          <Plus /> Add
        </Button>
      </div>
      {(problem || error) && (
        <span role="alert" className="text-xs text-danger">
          {problem ?? error}
        </span>
      )}
    </div>
  );
}
