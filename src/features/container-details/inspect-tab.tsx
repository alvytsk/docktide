import { RawJson } from "@/components/raw-json";
import { useLoadedContainer } from "./use-container";

/** The raw inspect JSON, unmasked, like `docker inspect` (spec M1 §10, D4). */
export function InspectTab() {
  return <RawJson raw={useLoadedContainer().raw} />;
}
