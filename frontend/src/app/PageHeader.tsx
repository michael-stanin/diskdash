import type { ReactNode } from "react";
import { Link } from "react-router";
import { StatusCluster } from "@/app/StatusCluster";

export interface Crumb {
  label: string;
  to?: string;
}

interface PageHeaderProps {
  title: ReactNode;
  subtitle?: ReactNode;
  breadcrumb?: Crumb[];
  actions?: ReactNode;
}

/** Page title block with breadcrumb, plus the global status cluster. */
export function PageHeader({ title, subtitle, breadcrumb, actions }: PageHeaderProps) {
  return (
    <header className="flex flex-wrap items-end gap-4">
      <div className="flex min-w-0 flex-col gap-1">
        {breadcrumb && breadcrumb.length > 0 && (
          <nav aria-label="Breadcrumb" className="flex items-center gap-2 text-[13px] text-muted-foreground">
            {breadcrumb.map((c, i) => (
              <span key={`${c.label}-${i}`} className="flex items-center gap-2">
                {i > 0 && <span aria-hidden>/</span>}
                {c.to ? (
                  <Link to={c.to} className="text-muted-foreground no-underline hover:text-foreground">
                    {c.label}
                  </Link>
                ) : (
                  <span aria-current="page" className="text-foreground">
                    {c.label}
                  </span>
                )}
              </span>
            ))}
          </nav>
        )}
        <h1 className="m-0 text-title font-semibold">{title}</h1>
        {subtitle && <p className="m-0 text-[13px] text-muted-foreground">{subtitle}</p>}
      </div>
      <div className="ml-auto flex flex-wrap items-center gap-3">
        <StatusCluster />
        {actions}
      </div>
    </header>
  );
}
