import { CheckSquare, HardDrive, LayoutGrid, Package, Settings, Sparkles, type LucideIcon } from "lucide-react";

export interface NavItem {
  to: string;
  label: string;
  icon: LucideIcon;
}

export const NAV_ITEMS: NavItem[] = [
  { to: "/", label: "Overview", icon: LayoutGrid },
  { to: "/drives", label: "Drives", icon: HardDrive },
  { to: "/recommendations", label: "Recommendations", icon: Sparkles },
  { to: "/apps", label: "Installed apps", icon: Package },
  { to: "/plans", label: "Plans", icon: CheckSquare },
  { to: "/settings", label: "Settings", icon: Settings },
];
