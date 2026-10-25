import {
  createHashHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  redirect,
  type RouterHistory,
} from "@tanstack/react-router";
import { ContainerPage } from "@/features/container-details/container-page";
import { InspectTab } from "@/features/container-details/inspect-tab";
import { OverviewTab } from "@/features/container-details/overview-tab";
import { LogsTab } from "@/features/logs/logs-tab";
import { StatsTab } from "@/features/stats/stats-tab";
import { ContainersPage } from "@/features/containers/containers-page";
import { ProjectLogsPage } from "@/features/project-logs/project-logs-page";
import { useDetailsTab, type DetailsTab } from "@/stores/details-tab";
import { NotFound, Shell } from "./shell";

// Code-based routes (spec M1 §5.1). Data loads through TanStack Query in
// components, not route loaders, so the connection gating stays in the pages.

const rootRoute = createRootRoute({ component: Shell, notFoundComponent: NotFound });

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/containers", replace: true });
  },
});

const containersRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "containers",
  component: Outlet,
});

const containerListRoute = createRoute({
  getParentRoute: () => containersRoute,
  path: "/",
  component: ContainersPage,
});

const containerRoute = createRoute({
  getParentRoute: () => containersRoute,
  path: "$id",
  component: ContainerPage,
});

const TAB_PATHS = {
  overview: "/containers/$id/overview",
  stats: "/containers/$id/stats",
  logs: "/containers/$id/logs",
  inspect: "/containers/$id/inspect",
} as const satisfies Record<DetailsTab, string>;

const containerIndexRoute = createRoute({
  getParentRoute: () => containerRoute,
  path: "/",
  beforeLoad: ({ params }) => {
    throw redirect({ to: TAB_PATHS[useDetailsTab.getState().tab], params, replace: true });
  },
});

const overviewRoute = createRoute({
  getParentRoute: () => containerRoute,
  path: "overview",
  component: OverviewTab,
  onEnter: () => useDetailsTab.getState().setTab("overview"),
});

const statsRoute = createRoute({
  getParentRoute: () => containerRoute,
  path: "stats",
  component: StatsTab,
  onEnter: () => useDetailsTab.getState().setTab("stats"),
});

const logsRoute = createRoute({
  getParentRoute: () => containerRoute,
  path: "logs",
  component: LogsTab,
  onEnter: () => useDetailsTab.getState().setTab("logs"),
});

const inspectRoute = createRoute({
  getParentRoute: () => containerRoute,
  path: "inspect",
  component: InspectTab,
  onEnter: () => useDetailsTab.getState().setTab("inspect"),
});

const projectLogsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "projects/$project/logs",
  component: ProjectLogsPage,
});

const routeTree = rootRoute.addChildren([
  indexRoute,
  containersRoute.addChildren([
    containerListRoute,
    containerRoute.addChildren([
      containerIndexRoute,
      overviewRoute,
      statsRoute,
      logsRoute,
      inspectRoute,
    ]),
  ]),
  projectLogsRoute,
]);

/** Hash history by default, since the app is served from bundled files. */
export function createAppRouter(history: RouterHistory = createHashHistory()) {
  // Never server-rendered; without this, tests in Node would take the server code path.
  return createRouter({ routeTree, history, isServer: false });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
