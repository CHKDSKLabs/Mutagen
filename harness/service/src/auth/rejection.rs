/// Why an auth verification rejected. Discriminant-only by design (ISC-001):
/// no payload, so an inbound bearer header can never get stapled into an
/// error variant and leaked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthRejectionReason {
    /// `Authorization` header absent.
    MissingHeader,
    /// Header present but not parseable as `Bearer <bytes>`.
    MalformedHeader,
    /// Header scheme wasn't `Bearer`.
    UnknownScheme,
    /// Header parsed but did not constant-time-match the configured secret.
    SecretMismatch,
    /// Verifier wired but no secret loaded — POL-A1 fail-closed.
    NotConfigured,
}
