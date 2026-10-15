import { createBrowserRouter } from "react-router";
import { AppShell } from "@/app/AppShell";
import { ComponentGallery } from "@/pages/ComponentGallery";
import { HomePage } from "@/pages/HomePage";
import { DriveDetailPage } from "@/pages/DriveDetailPage";
import { DrivesPage } from "@/pages/DrivesPage";
import { NotFoundPage } from "@/pages/placeholders";
import { PlansPage } from "@/pages/PlansPage";
import { TieringPage } from "@/pages/TieringPage";
import { RecommendationsPage } from "@/pages/RecommendationsPage";
import { AppsPage } from "@/pages/AppsPage";
import { DuplicatesPage } from "@/pages/DuplicatesPage";
import { RulesPage } from "@/pages/RulesPage";
import { SettingsPage } from "@/pages/SettingsPage";
import { SnapshotPage } from "@/pages/SnapshotPage";

export const routes = [
  {
    element: <AppShell />,
    children: [
      { path: "/", element: <HomePage /> },
      { path: "/drives", element: <DrivesPage /> },
      { path: "/drives/:id", element: <DriveDetailPage /> },
      { path: "/snapshots/:id", element: <SnapshotPage /> },
      { path: "/recommendations", element: <RecommendationsPage /> },
      { path: "/rules", element: <RulesPage /> },
      { path: "/apps", element: <AppsPage /> },
      { path: "/duplicates", element: <DuplicatesPage /> },
      { path: "/tiering", element: <TieringPage /> },
      { path: "/plans", element: <PlansPage /> },
      { path: "/settings", element: <SettingsPage /> },
      // Not linked from the UI; used by the Playwright visual smoke test.
      { path: "/dev/components", element: <ComponentGallery /> },
      { path: "*", element: <NotFoundPage /> },
    ],
  },
];

export function createRouter() {
  return createBrowserRouter(routes);
}
