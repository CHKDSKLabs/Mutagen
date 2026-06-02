use std::fmt;

use subtle::{Choice, ConstantTimeEq};

use crate::auth::secret::Secret;

/// Opaque inbound bearer-token bytes. Wraps `Vec<u8>` with a private field so
/// you cannot reach the bytes from outside this module, you cannot `Debug`
/// them (ISC-001), and you cannot compare them with `==` (ISC-002).
///
/// The compile-fail doc-test below is part of the type contract: if a future
/// edit derives `PartialEq` on this struct, this doc-test will start passing
/// the comparison and the test invariant flips loud.
///
/// ```compile_fail
/// use mutagen_service::auth::BearerToken;
/// let a = BearerToken::from_bytes(vec![1, 2, 3]);
/// let b = BearerToken::from_bytes(vec![1, 2, 3]);
/// let _ = a == b;
/// ```
pub struct BearerToken {
    bytes: Vec<u8>,
}

impl BearerToken {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Constant-time comparison against the configured shared [`Secret`].
    /// Returns `subtle::Choice` — not `bool` — so callers cannot accidentally
    /// short-circuit on it.
    pub fn verify_against(&self, secret: &Secret) -> Choice {
        self.bytes.ct_eq(secret.as_bytes())
    }
}

impl fmt::Debug for BearerToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted len={}>", self.bytes.len())
    }
}
