//! Origin helper for CLI-authored State Updates (ISC-007).
//!
//! Wraps `std::process::id()` into the canonical `Origin::Cli { pid }`
//! shape so callers don't reinvent the snake_case JSON every time.
//! Legacy log records (pre-0.4.0, no `origin` field) remain readable —
//! see MD-4 — but every new write from the CLI must come through here.

use mutagen_core::workflow::origin::Origin;

#[allow(dead_code)] // wired up at call-sites once core's State Update writers learn to take an Origin
pub fn cli_origin() -> Origin {
    Origin::Cli {
        pid: std::process::id(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_writes_origin_cli_pid() {
        let o = cli_origin();
        match o {
            Origin::Cli { pid } => assert_eq!(pid, std::process::id()),
            Origin::Service { .. } => panic!("cli_origin must mint a Cli variant, not Service"),
        }
        // Sanity: the Display form matches the on-the-wire shape ISC-007 mandates.
        assert_eq!(
            cli_origin().to_string(),
            format!("cli:{}", std::process::id())
        );
    }
}
