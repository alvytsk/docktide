import { create } from "zustand";
import type { ContainerFilter, StateFilter } from "@/features/containers/list";

type ContainerFilterStore = ContainerFilter & {
  setText: (text: string) => void;
  setState: (state: StateFilter) => void;
  clear: () => void;
};

const initial: ContainerFilter = { text: "", state: "all" };

/** Containers page filter. Not persisted in M0. */
export const useContainerFilter = create<ContainerFilterStore>()((set) => ({
  ...initial,
  setText: (text) => set({ text }),
  setState: (state) => set({ state }),
  clear: () => set(initial),
}));
