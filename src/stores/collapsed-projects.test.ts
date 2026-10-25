import { expect, it } from "vite-plus/test";
import { useCollapsedProjects } from "./collapsed-projects";

it("toggle collapses and expands one project only", () => {
  const { toggle } = useCollapsedProjects.getState();
  toggle("a");
  expect(useCollapsedProjects.getState().collapsed).toEqual({ a: true });
  toggle("b");
  toggle("a");
  expect(useCollapsedProjects.getState().collapsed).toEqual({ b: true });
});
