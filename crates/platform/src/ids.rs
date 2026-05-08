//! Typed ID newtypes, backed by UUID v7 for lexicographic ordering.

use uuid::Uuid;

/// Generate a new UUID v7 value.
#[must_use]
pub fn new_id() -> Uuid {
    Uuid::now_v7()
}

/// Macro to define a typed newtype ID. Each domain crate uses this to
/// create strongly-typed identifiers that do not silently interchange.
///
/// Generic doc strings are emitted on the generated struct + `new()`
/// so `#![warn(missing_docs)]` (set on every crate) doesn't fire
/// per-id. We deliberately don't interpolate `stringify!($name)` into
/// the doc text via `concat!` — rustfmt has a known oscillation bug
/// formatting `#[doc = concat!(...)]` inside macro arms, which would
/// break `cargo fmt --check` in CI.
#[macro_export]
macro_rules! define_id {
    ($name:ident) => {
        /// Typed UUID-v7 identifier. See `kubinate_platform::ids` for
        /// the macro that generates this newtype.
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            ::serde::Serialize,
            ::serde::Deserialize,
            ::sqlx::Type,
        )]
        #[sqlx(transparent)]
        pub struct $name(pub ::uuid::Uuid);

        impl $name {
            /// Mint a fresh value backed by `uuid::Uuid::now_v7`.
            #[must_use]
            pub fn new() -> Self {
                Self(::uuid::Uuid::now_v7())
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

// Example baseline ID types used across crates.
define_id!(UserId);
define_id!(OrganizationId);
