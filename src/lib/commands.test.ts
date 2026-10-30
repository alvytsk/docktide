import { expect, test } from "vite-plus/test";
import capability from "../../src-tauri/capabilities/default.json?raw";
import manifest from "../../src-tauri/build.rs?raw";

const files = import.meta.glob<string>("./ipc.ts", {
  query: "?raw",
  import: "default",
  eager: true,
});
const ipc = Object.values(files)[0] ?? "";

test("every command in ipc.ts is in the build manifest and the capability", () => {
  const names = new Set([...ipc.matchAll(/call<[^>]*>\(\s*"(\w+)"/g)].map((m) => m[1] ?? ""));
  // `${action}_container` takes the ContainerAction values.
  expect(ipc).toContain("`${action}_container`");
  for (const action of ["start", "stop", "restart"]) names.add(`${action}_container`);
  expect(names.size).toBeGreaterThan(10);
  for (const name of names) {
    expect(manifest, name).toContain(`"${name}"`);
    expect(capability, name).toContain(`"allow-${name.replaceAll("_", "-")}"`);
  }
});
