import { NavLink } from "react-router";
import { PanelLeftClose, PanelLeftOpen } from "lucide-react";
import { Logo } from "@/app/Logo";
import { NAV_ITEMS } from "@/app/nav";
import { Tooltip } from "@/components/ui/tooltip";
import { useSystem } from "@/api/queries";
import { cn } from "@/lib/cn";

interface SidebarProps {
  collapsed: boolean;
  onToggle: () => void;
}

export function Sidebar({ collapsed, onToggle }: SidebarProps) {
  const system = useSystem();
  return (
    <nav
      aria-label="Main"
      className={cn(
        "sticky top-0 flex h-screen shrink-0 flex-col gap-1 border-r border-border bg-sidebar py-5 transition-[width] duration-200",
        collapsed ? "w-[68px] px-3" : "w-[232px] px-3.5",
      )}
    >
      <div className={cn("flex items-center gap-2.5 px-2.5 pb-5", collapsed && "justify-center px-0")}>
        <Logo />
        {!collapsed && <span className="text-base font-semibold tracking-tight">diskdash</span>}
      </div>

      {NAV_ITEMS.map(({ to, label, icon: Icon }) => {
        const link = (
          <NavLink
            key={to}
            to={to}
            end={to === "/"}
            aria-label={collapsed ? label : undefined}
            className={({ isActive }) =>
              cn(
                "flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-muted-foreground no-underline transition-colors duration-150 hover:bg-hover hover:text-foreground",
                isActive && "bg-muted font-medium text-foreground",
                collapsed && "justify-center px-0",
              )
            }
          >
            <Icon className="size-4 shrink-0" />
            {!collapsed && label}
          </NavLink>
        );
        return collapsed ? (
          <Tooltip key={to} content={label} side="right">
            {link}
          </Tooltip>
        ) : (
          link
        );
      })}

      <div className="mt-auto flex flex-col gap-2">
        {!collapsed && system.data && (
          <div className="px-2.5 text-[11px] leading-relaxed text-subtle-foreground">
            v{system.data.version} · read-only
          </div>
        )}
        <button
          type="button"
          onClick={onToggle}
          aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          className={cn(
            "flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-[13px] text-muted-foreground hover:bg-hover hover:text-foreground",
            collapsed && "justify-center px-0",
          )}
        >
          {collapsed ? <PanelLeftOpen className="size-4" /> : <PanelLeftClose className="size-4" />}
          {!collapsed && "Collapse"}
        </button>
      </div>
    </nav>
  );
}
