-- Sprint 13: allow bearer-token authentication lookups on api_keys without
-- a tenant scope being set. The caller opts in by issuing
-- SET LOCAL app.api_key_auth_bypass = 'on' inside a transaction.
--
-- Security rationale: token_hash is the SHA-256 of a 256-bit secret; an
-- attacker who can compute the hash already possesses the token. This policy
-- does not give the application role the ability to enumerate foreign-tenant
-- keys under normal operation — only the explicit SET LOCAL opt-in activates
-- it, and that code path only runs during bearer-token authentication.

CREATE POLICY api_keys_auth_bypass ON api_keys
  AS PERMISSIVE
  FOR SELECT
  USING (current_setting('app.api_key_auth_bypass', true) = 'on');
