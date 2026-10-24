import { create } from "zustand";

/** Time under the pointer, shared by every chart on the page; `null` when no chart is hovered. */
export const useChartCursor = create<{
  time: number | null;
  setTime: (t: number | null) => void;
}>()((set) => ({
  time: null,
  setTime: (time) => set({ time }),
}));
