import { Sparkles } from "lucide-react";
import { Link } from "react-router";
import { PageHeader } from "@/app/PageHeader";
import { EmptyState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";

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
