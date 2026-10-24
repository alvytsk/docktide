import { create } from "zustand";

export type DetailsTab = "overview" | "stats" | "logs" | "inspect";

type DetailsTabStore = { tab: DetailsTab; setTab: (tab: DetailsTab) => void };

/** The last used details tab, reused when another container is opened. Session only. */
export const useDetailsTab = create<DetailsTabStore>()((set) => ({
  tab: "overview",
  setTab: (tab) => set({ tab }),
}));
