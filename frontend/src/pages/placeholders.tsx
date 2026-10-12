import { CheckSquare, Sparkles } from "lucide-react";
import { Link } from "react-router";
import { PageHeader } from "@/app/PageHeader";
import { EmptyState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";

export function RecommendationsPage() {
  return (
    <>
      <PageHeader title="Recommendations" subtitle="Ranked by space you could free versus risk." />
      <Card>
        <EmptyState
          icon={<Sparkles />}
          title="No recommendations yet"
          description="After a scan, diskdash suggests caches, leftovers and large old files you could remove or move. It never deletes anything itself."
          action={
            <Button asChild variant="secondary">
              <Link to="/">Go to overview</Link>
            </Button>
          }
        />
      </Card>
    </>
  );
}

export function PlansPage() {
  return (
    <>
      <PageHeader title="Plans" subtitle="Checklists of suggestions you decide to carry out yourself." />
      <Card>
        <EmptyState
          icon={<CheckSquare />}
          title="No plans yet"
          description="Add suggestions to a plan to get a checklist with the exact steps and the space each one frees."
        />
      </Card>
    </>
  );
}

export function NotFoundPage() {
  return (
    <>
      <PageHeader title="Page not found" />
      <Card>
        <EmptyState
          icon={<Sparkles />}
          title="There is nothing at this address"
          action={
            <Button asChild variant="primary">
              <Link to="/">Back to overview</Link>
            </Button>
          }
        />
      </Card>
    </>
  );
}
