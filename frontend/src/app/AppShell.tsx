import { useState } from "react";
import { Outlet } from "react-router";
import { AlertArea } from "@/app/AlertArea";
import { Sidebar } from "@/app/Sidebar";

const COLLAPSED_KEY = "diskdash.sidebar.collapsed";

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

export function AppShell() {
  const [collapsed, setCollapsed] = useState(readCollapsed);
  const toggle = () =>
    setCollapsed((c) => {
      try {
        localStorage.setItem(COLLAPSED_KEY, c ? "0" : "1");
      } catch {
        // Not persisted.
      }
      return !c;
    });

  return (
    <div className="flex min-h-screen bg-background text-foreground">
      <a
        href="#main"
        className="absolute top-2 left-2 z-50 -translate-y-20 rounded-md bg-primary px-3 py-2 text-primary-foreground focus:translate-y-0"
      >
        Skip to content
      </a>
      <Sidebar collapsed={collapsed} onToggle={toggle} />
      <main id="main" className="flex min-w-0 flex-1 flex-col">
        <div className="mx-auto flex w-full max-w-[1208px] flex-col gap-6 px-9 pt-7 pb-12">
          <AlertArea />
          <Outlet />
        </div>
      </main>
    </div>
  );
}
