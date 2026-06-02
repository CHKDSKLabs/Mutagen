use std::fmt;

/// Configured shared secret. Bytes are private; only [`Secret::secret_id`] is
/// safe to log.
///
/// ISC-001 / DSD-633: `Debug` prints `<secret id=...>`, never the bytes.
/// ISC-002 / INV-A2: `PartialEq` is intentionally NOT derived. The only way
/// to compare a token to this secret is through [`crate::auth::BearerToken::verify_against`],
/// which routes through `subtle::ConstantTimeEq`.
pub struct Secret {
    bytes: Vec<u8>,
    secret_id: String,
}

impl Secret {
    pub fn new(bytes: Vec<u8>, secret_id: String) -> Self {
        Self { bytes, secret_id }
    }

    pub fn secret_id(&self) -> &str {
        &self.secret_id
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<secret id={}>", self.secret_id)
    }
}
