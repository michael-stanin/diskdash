import { createBrowserRouter } from "react-router";
import { AppShell } from "@/app/AppShell";
import { ComponentGallery } from "@/pages/ComponentGallery";
import { HomePage } from "@/pages/HomePage";
import { DrivesPage, NotFoundPage, PlansPage, RecommendationsPage } from "@/pages/placeholders";
import { SettingsPage } from "@/pages/SettingsPage";

export const routes = [
  {
    element: <AppShell />,
    children: [
      { path: "/", element: <HomePage /> },
      { path: "/drives", element: <DrivesPage /> },
      { path: "/recommendations", element: <RecommendationsPage /> },
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
