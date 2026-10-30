import { expect, it } from "vite-plus/test";
import { useImageView } from "./image-view";

it("toggleSort flips on the same key and picks a direction for a new key", () => {
  const { toggleSort } = useImageView.getState();
  expect(useImageView.getState().sort).toEqual({ key: "uniqueSize", desc: true });
  toggleSort("uniqueSize");
  expect(useImageView.getState().sort).toEqual({ key: "uniqueSize", desc: false });
  toggleSort("name");
  expect(useImageView.getState().sort).toEqual({ key: "name", desc: false });
  toggleSort("created");
  expect(useImageView.getState().sort).toEqual({ key: "created", desc: true });
  toggleSort("size");
  expect(useImageView.getState().sort).toEqual({ key: "size", desc: true });
});

it("clear resets the text and usage filters but keeps the sort", () => {
  const view = useImageView.getState();
  view.setText("app");
  view.setUsage("unused");
  view.clear();
  const { text, usage, sort } = useImageView.getState();
  expect({ text, usage }).toEqual({ text: "", usage: "all" });
  expect(sort.key).toBe("size");
});
