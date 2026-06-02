use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use mutagen_service::config::{ConfigError, EnvOverrides, ServiceConfig, load_from};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_dir(tag: &str) -> PathBuf {
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("mutagen-cfg-{tag}-{stamp}-{nonce}"));
    std::fs::create_dir_all(&dir).expect("mk tmp dir");
    dir
}

fn write_config(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("service.toml");
    std::fs::write(&path, body).expect("write toml");
    path
}

#[test]
fn loads_pure_file_config() {
    let dir = unique_dir("file");
    let path = write_config(
        &dir,
        r#"
listen = "127.0.0.1:7777"
log_level = "debug"
secret_path = "/etc/mutagen/secret"
"#,
    );

    let cfg = load_from(&path, &EnvOverrides::default()).expect("load");

    assert_eq!(
        cfg,
        ServiceConfig {
            listen: "127.0.0.1:7777".into(),
            log_level: "debug".into(),
            secret_path: PathBuf::from("/etc/mutagen/secret"),
        }
    );
}

#[test]
fn env_overrides_take_priority() {
    let dir = unique_dir("override");
    let path = write_config(
        &dir,
        r#"
listen = "127.0.0.1:1111"
log_level = "warn"
secret_path = "/etc/mutagen/secret"
"#,
    );

    let env = EnvOverrides {
        listen: Some("0.0.0.0:9090".into()),
        log_level: Some("trace".into()),
        secret_path: Some(PathBuf::from("/run/secret/override")),
    };
    let cfg = load_from(&path, &env).expect("load");

    assert_eq!(cfg.listen, "0.0.0.0:9090");
    assert_eq!(cfg.log_level, "trace");
    assert_eq!(cfg.secret_path, PathBuf::from("/run/secret/override"));
}

#[test]
fn env_only_when_file_absent() {
    let dir = unique_dir("envonly");
    let missing = dir.join("nope.toml");

    let env = EnvOverrides {
        listen: Some("127.0.0.1:8888".into()),
        log_level: None,
        secret_path: Some(PathBuf::from("/var/secret")),
    };
    let cfg = load_from(&missing, &env).expect("load");

    assert_eq!(cfg.listen, "127.0.0.1:8888");
    assert_eq!(
        cfg.log_level, "info",
        "log_level falls back to info when nothing supplies it"
    );
    assert_eq!(cfg.secret_path, PathBuf::from("/var/secret"));
}

#[test]
fn missing_config_fails_closed() {
    let dir = unique_dir("failclosed");
    let missing = dir.join("nope.toml");

    let err = load_from(&missing, &EnvOverrides::default())
        .expect_err("loader must refuse when nothing is configured");

    match err {
        ConfigError::Missing {
            field,
            file_present,
            ..
        } => {
            assert!(
                !file_present,
                "file_present must be false for missing-file path"
            );
            assert!(
                field == "listen" || field == "secret_path",
                "first missing field should be listen or secret_path, got {field}"
            );
        }
        other => panic!("expected Missing, got {other:?}"),
    }
}

#[test]
fn missing_secret_fails_closed_even_when_listen_set() {
    let dir = unique_dir("nosecret");
    let path = write_config(&dir, r#"listen = "127.0.0.1:1234""#);

    let err = load_from(&path, &EnvOverrides::default())
        .expect_err("loader must refuse without a secret");

    match err {
        ConfigError::Missing {
            field,
            file_present,
            ..
        } => {
            assert_eq!(field, "secret_path");
            assert!(file_present);
        }
        other => panic!("expected Missing(secret_path), got {other:?}"),
    }
}

#[test]
fn rejects_malformed_toml() {
    let dir = unique_dir("bad");
    let path = write_config(&dir, "this = is = not = toml");

    let err = load_from(&path, &EnvOverrides::default()).expect_err("must reject bad toml");
    assert!(matches!(err, ConfigError::Parse { .. }), "got {err:?}");
}

#[test]
fn empty_env_value_does_not_override() {
    let dir = unique_dir("emptyenv");
    let path = write_config(
        &dir,
        r#"
listen = "127.0.0.1:5555"
secret_path = "/etc/mutagen/secret"
"#,
    );

    let env = EnvOverrides {
        listen: None,
        log_level: None,
        secret_path: None,
    };
    let cfg = load_from(&path, &env).expect("load");
    assert_eq!(cfg.listen, "127.0.0.1:5555");
}
