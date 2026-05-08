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
  { ignores: ['.svelte-kit/', 'build/', 'node_modules/'] }
];
