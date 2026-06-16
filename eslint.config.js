import js from "@eslint/js";
import ts from "typescript-eslint";
import svelte from "eslint-plugin-svelte";
import globals from "globals";

export default ts.config(
  js.configs.recommended,
  ...ts.configs.strictTypeChecked,
  ...svelte.configs["flat/recommended"],
  {
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
      parserOptions: {
        projectService: {
          allowDefaultProject: ["eslint.config.js", "svelte.config.js"],
        },
        tsconfigRootDir: import.meta.dirname,
        extraFileExtensions: [".svelte"],
      },
    },
  },
  {
    files: ["**/*.svelte"],
    languageOptions: {
      parserOptions: {
        parser: ts.parser,
      },
    },
  },
  // Config files live outside the tsconfig project — disable rules that need full type info
  {
    files: ["*.config.js", "*.config.ts", "e2e/**/*.ts", "e2e/**/*.mjs"],
    extends: [ts.configs.disableTypeChecked],
  },
  {
    ignores: [
      ".svelte-kit/",
      "build/",
      "dist/",
      "src-tauri/target/",
      "test-results/",
      "playwright-report/",
    ],
  },
);
