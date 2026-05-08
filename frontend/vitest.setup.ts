// Vitest pulls this in once before any test file. Currently only used
// to register `@testing-library/jest-dom` matchers (`toBeInTheDocument`,
// etc.) so component tests read naturally.

import '@testing-library/jest-dom/vitest';
