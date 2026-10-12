import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";

// Colour values live only in src/styles/tokens.css. Components must use
// token-backed Tailwind classes or CSS variables.
const colourLiteral = /#(?:[0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})\b|\b(?:rgb|rgba|hsl|hsla)\(/;
const noColourLiterals = {
  "no-restricted-syntax": [
    "error",
    {
      selector: `Literal[value=${colourLiteral}]`,
      message: "Use a design token (src/styles/tokens.css) instead of a colour literal.",
    },
    {
      selector: `TemplateElement[value.raw=${colourLiteral}]`,
      message: "Use a design token (src/styles/tokens.css) instead of a colour literal.",
    },
  ],
};

export default tseslint.config(
  { ignores: ["dist", "node_modules", "src/api/types", "test-results", "playwright-report"] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ["**/*.{ts,tsx}"],
    languageOptions: { ecmaVersion: 2022, globals: { ...globals.browser, ...globals.node } },
    plugins: { "react-hooks": reactHooks, "react-refresh": reactRefresh },
    rules: {
      ...reactHooks.configs.recommended.rules,
      // Providers export hooks next to components by design.
      "react-refresh/only-export-components": "off",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }],
    },
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/**/*.test.{ts,tsx}"],
    rules: noColourLiterals,
  },
);
