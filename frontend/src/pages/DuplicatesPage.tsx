import { PageHeader } from "@/app/PageHeader";
import { useDrives } from "@/api/queries";
import { DuplicatesView } from "@/components/duplicates/DuplicatesView";

export function DuplicatesPage() {
  const drives = useDrives();
  const letters = (drives.data?.drives ?? []).flatMap((d) => d.letters.map((l) => ({ letter: l })));
  return (
    <>
      <PageHeader
        title="Duplicates"
        subtitle="Exact copies of the same file, with the space they waste. diskdash only suggests which copy to keep: you remove the others yourself."
      />
      <DuplicatesView drives={letters} />
    </>
  );
}
