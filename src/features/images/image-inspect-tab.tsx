import { RawJson } from "@/components/raw-json";
import { useLoadedImage } from "./use-images";

export function ImageInspectTab() {
  return <RawJson raw={useLoadedImage().raw} />;
}
