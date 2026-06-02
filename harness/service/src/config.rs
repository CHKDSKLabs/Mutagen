use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const ENV_LISTEN: &str = "MUTAGEN_SERVICE_LISTEN";
pub const ENV_LOG_LEVEL: &str = "MUTAGEN_SERVICE_LOG_LEVEL";
pub const ENV_SECRET_PATH: &str = "MUTAGEN_SERVICE_SECRET_PATH";

pub const DEFAULT_CONFIG_RELATIVE: &str = ".mutagen/service.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    pub listen: String,
    pub log_level: String,
    pub secret_path: PathBuf,
}

#[derive(Debug, Default, Deserialize)]
struct ServiceConfigFile {
    listen: Option<String>,
    log_level: Option<String>,
    secret_path: Option<PathBuf>,
}

#[derive(Debug, Default, Clone)]
pub struct EnvOverrides {
    pub listen: Option<String>,
    pub log_level: Option<String>,
    pub secret_path: Option<PathBuf>,
}

impl EnvOverrides {
    pub fn from_process_env() -> Self {
        Self {
            listen: read_env(ENV_LISTEN),
            log_level: read_env(ENV_LOG_LEVEL),
            secret_path: read_env(ENV_SECRET_PATH).map(PathBuf::from),
        }
    }
}

fn read_env(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

#[derive(Debug)]
pub enum ConfigError {
    FileRead {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    Missing {
        field: &'static str,
        env: &'static str,
        path: PathBuf,
        file_present: bool,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::FileRead { path, source } => {
                write!(
                    f,
                    "could not read service config at {}: {source}",
                    path.display()
                )
            }
            ConfigError::Parse { path, source } => {
                write!(
                    f,
                    "service config at {} is not valid TOML: {source}",
                    path.display()
                )
            }
            ConfigError::Missing {
                field,
                env,
                path,
                file_present,
            } => {
                let where_from = if *file_present {
                    format!("present in {} or via {env}", path.display())
                } else {
                    format!("file {} missing and {env} unset", path.display())
                };
                write!(
                    f,
                    "service refuses to start: required field `{field}` not configured ({where_from})"
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::FileRead { source, .. } => Some(source),
            ConfigError::Parse { source, .. } => Some(source),
            ConfigError::Missing { .. } => None,
        }
    }
}

pub fn load_from(config_path: &Path, env: &EnvOverrides) -> Result<ServiceConfig, ConfigError> {
    let (file, file_present) = match std::fs::read_to_string(config_path) {
        Ok(text) => {
            let parsed: ServiceConfigFile =
                toml::from_str(&text).map_err(|source| ConfigError::Parse {
                    path: config_path.to_path_buf(),
                    source: Box::new(source),
                })?;
            (parsed, true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            (ServiceConfigFile::default(), false)
        }
        Err(source) => {
            return Err(ConfigError::FileRead {
                path: config_path.to_path_buf(),
                source,
            });
        }
    };

    let listen = env
        .listen
        .clone()
        .or(file.listen)
        .ok_or_else(|| ConfigError::Missing {
            field: "listen",
            env: ENV_LISTEN,
            path: config_path.to_path_buf(),
            file_present,
        })?;

    let secret_path = env
        .secret_path
        .clone()
        .or(file.secret_path)
        .ok_or_else(|| ConfigError::Missing {
            field: "secret_path",
            env: ENV_SECRET_PATH,
            path: config_path.to_path_buf(),
            file_present,
        })?;

    let log_level = env
        .log_level
        .clone()
        .or(file.log_level)
        .unwrap_or_else(|| "info".to_string());

    Ok(ServiceConfig {
        listen,
        log_level,
        secret_path,
    })
}

pub fn load_for_project(project_root: &Path) -> Result<ServiceConfig, ConfigError> {
    let path = project_root.join(DEFAULT_CONFIG_RELATIVE);
    load_from(&path, &EnvOverrides::from_process_env())
}
