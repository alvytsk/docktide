import { create } from "zustand";
import type { ImageSort, SortKey, UsageFilter } from "@/features/images/list";

type ImageViewStore = {
  text: string;
  usage: UsageFilter;
  sort: ImageSort;
  setText: (text: string) => void;
  setUsage: (usage: UsageFilter) => void;
  toggleSort: (key: SortKey) => void;
  clear: () => void;
};

/** Images page sort and filters, kept for the session. */
export const useImageView = create<ImageViewStore>()((set) => ({
  text: "",
  usage: "all",
  sort: { key: "uniqueSize", desc: true },
  setText: (text) => set({ text }),
  setUsage: (usage) => set({ usage }),
  toggleSort: (key) =>
    set(({ sort }) => ({
      sort: { key, desc: sort.key === key ? !sort.desc : key !== "name" },
    })),
  clear: () => set({ text: "", usage: "all" }),
}));
