import { createMemoryHistory } from "@tanstack/react-router";
import { beforeEach, describe, expect, it, test, vi } from "vite-plus/test";
import { useDetailsTab } from "@/stores/details-tab";
import { createAppRouter } from "./router";

// The client router reads `window.origin`; Node has no window.
vi.stubGlobal("window", {});

async function landOn(path: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  await router.load();
  return {
    path: router.state.location.pathname,
    routes: router.state.matches.map((match) => match.routeId),
    // The match that renders the not-found component is flagged `_notFound`;
    // the router has no public equivalent without rendering.
    // oxlint-disable-next-line no-underscore-dangle
    notFound: router.state.matches.some((match) => match._notFound),
  };
}

describe("routes", () => {
  beforeEach(() => useDetailsTab.setState({ tab: "overview" }));

  test("/ redirects to the container list", async () => {
    expect(await landOn("/")).toMatchObject({ path: "/containers", notFound: false });
  });

  test("a container opens on overview by default", async () => {
    expect((await landOn("/containers/abc")).path).toBe("/containers/abc/overview");
  });

  test("a container opens on the last used tab", async () => {
    useDetailsTab.setState({ tab: "inspect" });
    expect((await landOn("/containers/abc")).path).toBe("/containers/abc/inspect");
  });

  it("redirects /containers/$id to the stats tab when it was used last", async () => {
    useDetailsTab.getState().setTab("stats");
    expect((await landOn("/containers/abc")).path).toBe("/containers/abc/stats");
  });

  test("the logs tab is remembered too", async () => {
    await landOn("/containers/abc/logs");
    expect((await landOn("/containers/def")).path).toBe("/containers/def/logs");
  });

  test("entering a tab remembers it", async () => {
    await landOn("/containers/abc/inspect");
    expect(useDetailsTab.getState().tab).toBe("inspect");
  });

  test("tabs match their routes", async () => {
    expect((await landOn("/containers/abc/overview")).routes).toContain("/containers/$id/overview");
  });

  test("a project's logs page is a route", async () => {
    expect(await landOn("/projects/shop/logs")).toMatchObject({
      path: "/projects/shop/logs",
      notFound: false,
    });
  });

  test("unknown paths reach the not-found component", async () => {
    expect((await landOn("/nope")).notFound).toBe(true);
    expect((await landOn("/containers/abc/nope")).notFound).toBe(true);
  });
});
