//! MFA recovery-code generation + hashing helpers (Sprint 4 ticket
//! 05).
//!
//! ## Format
//!
//! Each code is **10 characters** drawn from **Crockford Base32**
//! (no `I`, `L`, `O`, `U` — the four most-confused glyphs). 32¹⁰ ≈
//! 10¹⁵ possibilities; for one-shot codes this is overkill from a
//! brute-force standpoint and tuned for typability instead. An
//! operator dictating a code on a runbook escalation path can
//! pronounce every glyph unambiguously.
//!
//! Codes are rendered with a hyphen between two 5-char halves
//! (`AB12C-D3E4F`) for readability; the consume path strips the
//! hyphen + uppercases before hashing so users who type lowercase
//! or omit the separator still match.
//!
//! ## Hashing
//!
//! Storage is SHA-256 of the canonical (uppercase, no-hyphen) form.
//! The repository layer round-trips `[u8; 32]` so a compromised DB
//! row can't be replayed.

use sha2::{Digest, Sha256};

/// Crockford Base32 alphabet — no I, L, O, U.
const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// How many chars per recovery code. 10 chars × 5 bits/char = 50
/// bits of entropy per code. Plenty for one-shot use.
const CODE_LEN: usize = 10;

/// How many codes per batch. The `5-webauthn-for-owners.md` `DoD`
/// specifies 10; we follow the established pattern (GitHub /
/// GitLab / Auth0 all default to 8–10).
pub const BATCH_SIZE: usize = 10;

/// Generate a single recovery code drawing entropy from the supplied
/// CSPRNG. Format: `XXXXX-XXXXX` (Crockford-Base32 chars,
/// hyphen-separated halves).
pub fn generate_one(rng: &mut impl rand::RngCore) -> String {
    use rand::Rng;
    let mut chars = String::with_capacity(CODE_LEN + 1);
    for i in 0..CODE_LEN {
        let idx = rng.gen_range(0..ALPHABET.len());
        chars.push(ALPHABET[idx] as char);
        if i == CODE_LEN / 2 - 1 {
            chars.push('-');
        }
    }
    chars
}

/// Generate a fresh batch of [`BATCH_SIZE`] codes.
pub fn generate_batch(rng: &mut impl rand::RngCore) -> Vec<String> {
    (0..BATCH_SIZE).map(|_| generate_one(rng)).collect()
}

/// Hash a code into the canonical 32-byte form the repository
/// stores. Strips hyphens + uppercases first so input variation
/// from the user doesn't break matching.
#[must_use]
pub fn hash(code: &str) -> [u8; 32] {
    let canonical: String = code
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_uppercase)
        .collect();
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn generated_code_has_canonical_format() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let code = generate_one(&mut rng);
        assert_eq!(code.len(), CODE_LEN + 1);
        assert_eq!(code.chars().nth(5), Some('-'));
        // Every non-hyphen char is in Crockford Base32.
        for c in code.chars().filter(|c| *c != '-') {
            assert!(
                ALPHABET.contains(&(c as u8)),
                "char {c} is not in Crockford Base32"
            );
        }
    }

    #[test]
    fn generate_batch_yields_unique_codes() {
        // Probabilistically near-certain at our entropy levels;
        // assert it as a regression test against an alphabet shrink
        // or generator change that would silently collide more.
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let batch = generate_batch(&mut rng);
        assert_eq!(batch.len(), BATCH_SIZE);
        let mut seen = std::collections::HashSet::new();
        for code in &batch {
            assert!(seen.insert(code.clone()), "duplicate in batch: {code}");
        }
    }

    #[test]
    fn hash_strips_hyphens_and_uppercases() {
        // The user-friendly path: a user types the code without the
        // hyphen, or in lowercase, or with whitespace; the hash
        // matches the canonical form. Critical for the redemption
        // path — losing this property means false-negative match on
        // 50%+ of human-typed codes.
        let canonical = hash("AB12C-D3E4F");
        let no_hyphen = hash("AB12CD3E4F");
        let lower = hash("ab12c-d3e4f");
        let spaces = hash(" AB12C-D3E4F ");
        assert_eq!(canonical, no_hyphen);
        assert_eq!(canonical, lower);
        assert_eq!(canonical, spaces);
    }

    #[test]
    fn distinct_codes_hash_differently() {
        // Sanity check the hash isn't truncating somewhere subtle.
        let a = hash("AB12C-D3E4F");
        let b = hash("AB12C-D3E4G");
        assert_ne!(a, b);
    }
}
