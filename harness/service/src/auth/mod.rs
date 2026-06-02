pub mod bearer;
pub mod rejection;
pub mod secret;

pub use bearer::BearerToken;
pub use rejection::AuthRejectionReason;
pub use secret::Secret;

use std::fmt;
use std::path::PathBuf;

use crate::config::ServiceConfig;

/// Env var consulted before falling back to `config.secret_path`. Reading the
/// secret straight from the environment is intended for ops/CI runs; the
/// file path is the canonical disk source.
pub const ENV_SECRET: &str = "MUTAGEN_SERVICE_SECRET";

#[derive(Debug, Default, Clone)]
pub struct SecretEnv {
    pub value: Option<String>,
}

impl SecretEnv {
    pub fn from_process_env() -> Self {
        let value = std::env::var(ENV_SECRET)
            .ok()
            .filter(|v| !v.trim().is_empty());
        Self { value }
    }
}

#[derive(Debug, Clone)]
pub enum SecretSource {
    Env,
    File(PathBuf),
}

#[derive(Debug)]
pub enum SecretLoadError {
    NotConfigured {
        env: &'static str,
        path: PathBuf,
    },
    Empty {
        source: SecretSource,
    },
    FileRead {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for SecretLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretLoadError::NotConfigured { env, path } => write!(
                f,
                "service refuses to start: no shared secret configured (set {env} or place bytes at {})",
                path.display()
            ),
            SecretLoadError::Empty { source } => match source {
                SecretSource::Env => write!(
                    f,
                    "service refuses to start: {ENV_SECRET} is empty after trim"
                ),
                SecretSource::File(p) => write!(
                    f,
                    "service refuses to start: secret file {} is empty after trim",
                    p.display()
                ),
            },
            SecretLoadError::FileRead { path, source: _ } => write!(
                f,
                "service refuses to start: could not read secret file {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for SecretLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SecretLoadError::FileRead { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn trim_trailing(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 {
        match bytes[end - 1] {
            b'\n' | b'\r' | b' ' | b'\t' => end -= 1,
            _ => break,
        }
    }
    &bytes[..end]
}

/// Load the shared secret with explicit env overrides (testable form).
/// ISC-003 / POL-A1: every failure path returns `Err`; never silently
/// produces a `Secret`. Caller (startup) is expected to treat any `Err`
/// as fail-closed and abort the bind.
pub fn load_secret(config: &ServiceConfig, env: &SecretEnv) -> Result<Secret, SecretLoadError> {
    if let Some(raw) = env.value.as_deref() {
        let trimmed = trim_trailing(raw.as_bytes());
        if trimmed.is_empty() {
            return Err(SecretLoadError::Empty {
                source: SecretSource::Env,
            });
        }
        return Ok(Secret::new(trimmed.to_vec(), format!("env:{ENV_SECRET}")));
    }

    match std::fs::read(&config.secret_path) {
        Ok(raw) => {
            let trimmed = trim_trailing(&raw);
            if trimmed.is_empty() {
                return Err(SecretLoadError::Empty {
                    source: SecretSource::File(config.secret_path.clone()),
                });
            }
            Ok(Secret::new(
                trimmed.to_vec(),
                format!("file:{}", config.secret_path.display()),
            ))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err(SecretLoadError::NotConfigured {
                env: ENV_SECRET,
                path: config.secret_path.clone(),
            })
        }
        Err(source) => Err(SecretLoadError::FileRead {
            path: config.secret_path.clone(),
            source,
        }),
    }
}

/// Process-env convenience wrapper. Startup calls this; tests call
/// [`load_secret`] with an explicit [`SecretEnv`] so they don't have to
/// mutate global env state.
pub fn load_secret_from_process(config: &ServiceConfig) -> Result<Secret, SecretLoadError> {
    load_secret(config, &SecretEnv::from_process_env())
}
