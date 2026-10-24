// The project's ESLint config. The gate's rules are in sadr.eslint.mjs, which `sadr init`
// rewrites; the project's own go after them here, so they win.
import { defineConfig } from "eslint/config";
import sadr from "./sadr.eslint.mjs";

export default defineConfig([
  sadr,
  // The project's own settings, such as { rules: { ... } }, go here.
]);
