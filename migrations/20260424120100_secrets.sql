-- =============================================================================
-- 20260424120100_secrets.sql
-- =============================================================================
-- Envelope-encrypted secret storage (ADR-0007, Phases 0–2).
--
-- Every row stores one customer secret (Hetzner tokens, kubeconfigs, etc.).
-- Encryption is envelope: a random per-record DEK encrypts the plaintext, and
-- the DEK itself is encrypted under the KEK that the application loads from
-- `KUBINATE_KEK`. The KEK never lives in the database.
--
-- pgcrypto's `pgp_sym_encrypt` is the primitive used by both layers; it
-- provides authenticated encryption via OpenPGP and generates its own IV per
-- call, so storing only the ciphertext bytes (and no IV column) is correct.
--
-- Consumers: `kubinate_platform::secrets::PgcryptoStore`. Domain tables
-- (hetzner_credentials, cluster_kubeconfigs) reference rows here by UUID.
-- =============================================================================

CREATE TABLE secrets (
    id              UUID PRIMARY KEY,
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    ciphertext      BYTEA NOT NULL,
    wrapped_dek     BYTEA NOT NULL,
    algorithm       TEXT  NOT NULL DEFAULT 'pgp_sym_v1',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    version         BIGINT NOT NULL DEFAULT 1
);

CREATE INDEX secrets_org_idx ON secrets (organization_id);

ALTER TABLE secrets ENABLE ROW LEVEL SECURITY;
ALTER TABLE secrets FORCE  ROW LEVEL SECURITY;

CREATE POLICY secrets_tenant_scope ON secrets
  USING       (organization_id = app_current_tenant_id())
  WITH CHECK  (organization_id = app_current_tenant_id());
