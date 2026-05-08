/**
 * Tiny indirection around `window.location.assign` so tests can mock
 * the navigation without fighting jsdom's non-configurable
 * `location` property.
 */
export function externalNavigate(url: string): void {
  // SSR safety: if this ever runs server-side, it's a no-op.
  if (typeof window !== 'undefined') {
    window.location.assign(url);
  }
}
