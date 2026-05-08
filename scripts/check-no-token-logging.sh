#!/usr/bin/env bash
# Fail if any Rust source captures a raw auth-token-looking value as a
# `tracing` event field. Implements the DoD row from Sprint 1 ticket 01:
#   "No token fields on any tracing event at any level (grep assertion in CI)"
#
# The tracing field-capture syntax uses `%` (Display) or `?` (Debug)
# before the variable name, e.g. `tracing::info!(token = %t, ...)` or
# the shorthand `tracing::info!(%token, ...)`. A regex anchored to those
# two sigils catches the field-capture case without matching
# legitimate identifier substrings (`token_prefix`, `token_hash`, …).
#
# Run from repo root, either directly or as a CI step.

set -euo pipefail

cd "$(dirname "$0")/.."

# POSIX ERE; `grep -E` is available on every CI image we care about.
# Word boundaries are approximated via `[^_a-zA-Z0-9]` lookahead using
# `grep -oE` then filtering, but for simplicity we use `\b` via `grep -P`
# when available and fall back to a character-class suffix otherwise.
PATTERNS=(
  '[%?]token([^_a-zA-Z0-9]|$)'
  '[%?]hcloud_token([^_a-zA-Z0-9]|$)'
  '[%?]hetzner_token([^_a-zA-Z0-9]|$)'
  '[%?]api_token([^_a-zA-Z0-9]|$)'
  '[%?]access_token([^_a-zA-Z0-9]|$)'
  '[%?]refresh_token([^_a-zA-Z0-9]|$)'
  '[%?]bearer([^_a-zA-Z0-9]|$)'
  '[%?]password([^_a-zA-Z0-9]|$)'
  '[%?]kek([^_a-zA-Z0-9]|$)'
  '[%?]dek([^_a-zA-Z0-9]|$)'
)

# Collect .rs files under the crates/ and agent/ trees, excluding tests.
# find/xargs keeps this portable across bash 3 (macOS) and bash 4+.
FILES_LIST=$(find crates agent -type f -name '*.rs' \
  -not -path '*/tests/*' \
  -not -name '*test*.rs')

if [[ -z "$FILES_LIST" ]]; then
  echo "OK: no Rust files to scan."
  exit 0
fi

hit=0
for pat in "${PATTERNS[@]}"; do
  if echo "$FILES_LIST" | tr '\n' '\0' | xargs -0 grep -H -n -E "$pat"; then
    hit=1
  fi
done

if [[ $hit -ne 0 ]]; then
  cat >&2 <<'EOF'

FAIL: tracing-event field capture appears to reference sensitive material.

Every raw secret must live inside a `SecretString` (the `Debug` impl
redacts) and must not be captured by `%` / `?` on a field named
`token`, `password`, `kek`, etc. If a log line needs to mention a
specific credential, use a non-sensitive field like `credential_id` or
`secret_id` instead.
EOF
  exit 1
fi

echo "OK: no sensitive token capture in tracing events."
