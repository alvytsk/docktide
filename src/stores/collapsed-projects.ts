import { create } from "zustand";

type CollapsedProjects = {
  collapsed: Record<string, true>;
  toggle: (project: string) => void;
};

/** Collapsed Compose groups in the container list, by project name. Session only. */
export const useCollapsedProjects = create<CollapsedProjects>()((set) => ({
  collapsed: {},
  toggle: (project) =>
    set(({ collapsed: { [project]: was, ...rest } }) => ({
      collapsed: was ? rest : { ...rest, [project]: true },
    })),
}));
