import { createBrowserRouter, RouterProvider } from "react-router";
import { AppShell } from "@/app/app-shell";

function RouteLoading() {
  return (
    <main aria-busy="true" aria-label="正在加载页面" className="page-shell">
      <div className="skeleton skeleton-title" />
      <div className="skeleton skeleton-card" />
    </main>
  );
}

const createAppRouter = () =>
  createBrowserRouter([
    {
      path: "/",
      Component: AppShell,
      HydrateFallback: RouteLoading,
      children: [
        { index: true, lazy: () => import("@/app/routes/dashboard") },
        { path: "devices", lazy: () => import("@/app/routes/devices") },
        {
          path: "integrations",
          lazy: () => import("@/app/routes/integrations"),
        },
        { path: "themes", lazy: () => import("@/app/routes/themes") },
        { path: "preview", lazy: () => import("@/app/routes/preview") },
        { path: "settings", lazy: () => import("@/app/routes/settings") },
      ],
    },
    {
      path: "*",
      lazy: () => import("@/app/routes/not-found"),
    },
  ]);

export default function AppRouter() {
  return <RouterProvider router={createAppRouter()} />;
}
