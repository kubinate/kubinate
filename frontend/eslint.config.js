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
      globals: {
        ...globals.browser,
        ...globals.node,
        // WebAuthn DOM types live in `lib.dom.d.ts` and TypeScript
        // sees them through tsconfig, but eslint's `no-undef` rule
        // doesn't. The casts in `src/lib/api/passkey.ts` and
        // `src/routes/app/settings/security/+page.svelte` need
        // these to lint clean.
        PublicKeyCredential: 'readonly',
        PublicKeyCredentialType: 'readonly',
        PublicKeyCredentialCreationOptions: 'readonly',
        PublicKeyCredentialRequestOptions: 'readonly',
        AuthenticatorAttestationResponse: 'readonly',
        AuthenticatorAssertionResponse: 'readonly',
        CredentialCreationOptions: 'readonly',
        CredentialRequestOptions: 'readonly'
      }
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
  {
    // shadcn-svelte components use `let { ...rest } = $props()` throughout.
    // The `custom_element_props_identifier` sub-rule of `svelte/valid-compile`
    // fires on rest-element props because the linter can't infer the prop list
    // for custom elements — but these components are not custom elements and
    // the pattern is intentional. Disable the rule for the ui/ subtree only.
    files: ['src/lib/components/ui/**/*.svelte'],
    rules: {
      'svelte/valid-compile': 'off'
    }
  },
  { ignores: ['.svelte-kit/', 'build/', 'node_modules/'] }
];
