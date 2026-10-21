import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite-plus";

const host = process.env.TAURI_DEV_HOST;

const generated = [
  "dist/**",
  "src-tauri/**",
  "target/**",
  "src/bindings/**",
  "crates/*/testdata/**",
];

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },

  // Tauri expects a fixed port and must see Rust errors in the terminal.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    // WebKitGTK 2.40+ (Tauri 2 Linux baseline) supports ES2022.
    target: "es2022",
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
  },

  test: {
    include: ["src/**/*.test.{ts,tsx}"],
    passWithNoTests: true,
  },

  lint: {
    ignorePatterns: generated,
    plugins: ["react", "typescript", "oxc", "unicorn", "import"],
    categories: { correctness: "error", suspicious: "warn" },
    options: { typeAware: true, typeCheck: true },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",
      "typescript/no-floating-promises": "error",
      "typescript/no-misused-promises": "error",
      "typescript/switch-exhaustiveness-check": "error",
      // The automatic JSX runtime does not need React in scope.
      "react/react-in-jsx-scope": "off",
      // Side-effect imports are how CSS gets into the bundle.
      "import/no-unassigned-import": ["warn", { allow: ["**/*.css"] }],
    },
  },

  fmt: {
    ignorePatterns: generated,
  },
});
