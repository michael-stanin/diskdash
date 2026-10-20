import { ArrowLeftRight, CheckSquare, Copy, HardDrive, Bell, LayoutGrid, LineChart, Package, Settings, Sparkles, type LucideIcon } from "lucide-react";

export interface NavItem {
  to: string;
  label: string;
  icon: LucideIcon;
}

export const NAV_ITEMS: NavItem[] = [
  { to: "/", label: "Overview", icon: LayoutGrid },
  { to: "/drives", label: "Drives", icon: HardDrive },
  { to: "/history", label: "History", icon: LineChart },
  { to: "/alerts", label: "Alerts", icon: Bell },
  { to: "/recommendations", label: "Recommendations", icon: Sparkles },
  { to: "/apps", label: "Installed apps", icon: Package },
  { to: "/duplicates", label: "Duplicates", icon: Copy },
  { to: "/tiering", label: "Tiering", icon: ArrowLeftRight },
  { to: "/plans", label: "Plans", icon: CheckSquare },
  { to: "/settings", label: "Settings", icon: Settings },
];
