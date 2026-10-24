// The project's ESLint config. The gate's rules are in plinth.eslint.mjs, which `plinth init`
// rewrites; the project's own go after them here, so they win.
import { defineConfig } from "eslint/config";
import plinth from "./plinth.eslint.mjs";

export default defineConfig([
  plinth,
  // The project's own settings, such as { rules: { ... } }, go here.
]);
