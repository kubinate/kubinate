import js from '@eslint/js';
import svelte from 'eslint-plugin-svelte';
import prettier from 'eslint-config-prettier';
import globals from 'globals';
import tseslint from 'typescript-eslint';

export default [
  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...svelte.configs['flat/recommended'],
  prettier,
  ...svelte.configs['flat/prettier'],
  {
    languageOptions: {
      globals: { ...globals.browser, ...globals.node }
    }
  },
  {
    // Svelte's flat-recommended config sets `svelte-eslint-parser` as
    // the parser for *.svelte files; we additionally point its inner
    // <script lang="ts"> parser at typescript-eslint so generic and
    // type-only syntax parses correctly.
    files: ['**/*.svelte'],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser
      }
    }
  },
  { ignores: ['.svelte-kit/', 'build/', 'node_modules/'] }
];
