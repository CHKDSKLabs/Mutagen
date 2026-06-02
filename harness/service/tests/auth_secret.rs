use std::path::PathBuf;

use mutagen_service::auth::{self, BearerToken, Secret, SecretEnv, SecretLoadError};
use mutagen_service::config::ServiceConfig;

fn cfg_with_secret_path(p: PathBuf) -> ServiceConfig {
    ServiceConfig {
        listen: "127.0.0.1:0".to_string(),
        log_level: "info".to_string(),
        secret_path: p,
    }
}

#[test]
fn bearer_token_debug_redacts() {
    let token = BearerToken::from_bytes(b"super-secret-bytes".to_vec());
    let rendered = format!("{token:?}");
    assert!(
        rendered.contains("<redacted len=18>"),
        "expected redacted Debug, got: {rendered}"
    );
    assert!(
        !rendered.contains("super-secret-bytes"),
        "Debug leaked raw bytes: {rendered}"
    );
}

#[test]
fn secret_debug_redacts() {
    let secret = Secret::new(b"hunter2".to_vec(), "test:fixture".to_string());
    let rendered = format!("{secret:?}");
    assert!(
        rendered.contains("<secret id=test:fixture>"),
        "expected `<secret id=...>` form, got: {rendered}"
    );
    assert!(
        !rendered.contains("hunter2"),
        "Debug leaked secret bytes: {rendered}"
    );
}

#[test]
fn compare_uses_constant_time() {
    let secret = Secret::new(
        b"correct-horse-battery-staple".to_vec(),
        "test:fixture".to_string(),
    );

    let good = BearerToken::from_bytes(b"correct-horse-battery-staple".to_vec());
    let bad_same_len = BearerToken::from_bytes(b"wrong-horse--battery-staple_".to_vec());
    let bad_diff_len = BearerToken::from_bytes(b"short".to_vec());

    assert!(bool::from(good.verify_against(&secret)));
    assert!(!bool::from(bad_same_len.verify_against(&secret)));
    assert!(!bool::from(bad_diff_len.verify_against(&secret)));
}

#[test]
fn missing_secret_fails_startup() {
    let cfg = cfg_with_secret_path(PathBuf::from(
        "/this/path/does/not/exist/anywhere-mutagen-auth-001",
    ));
    let env = SecretEnv { value: None };
    let result = auth::load_secret(&cfg, &env);
    match result {
        Err(SecretLoadError::NotConfigured { .. }) => {}
        other => panic!("expected NotConfigured fail-closed, got: {other:?}"),
    }
}

#[test]
fn empty_env_secret_fails_closed() {
    let cfg = cfg_with_secret_path(PathBuf::from(
        "/this/path/does/not/exist/anywhere-mutagen-auth-001",
    ));
    let env = SecretEnv {
        value: Some("   \n\t".to_string()),
    };
    match auth::load_secret(&cfg, &env) {
        Err(SecretLoadError::Empty { .. }) | Err(SecretLoadError::NotConfigured { .. }) => {}
        other => panic!("blank env should fail closed, got: {other:?}"),
    }
}

#[test]
fn env_secret_trimmed_and_labelled() {
    let cfg = cfg_with_secret_path(PathBuf::from("/nope"));
    let env = SecretEnv {
        value: Some("hunter2\n".to_string()),
    };
    let secret = auth::load_secret(&cfg, &env).expect("env path should populate secret");
    assert_eq!(secret.len(), 7);
    assert!(secret.secret_id().starts_with("env:"));
    let debug = format!("{secret:?}");
    assert!(!debug.contains("hunter2"));
}

#[test]
fn file_secret_loaded_when_env_absent() {
    let dir = std::env::temp_dir().join("mutagen-auth-001-test");
    std::fs::create_dir_all(&dir).expect("setup tmpdir");
    let path = dir.join("secret.bytes");
    std::fs::write(&path, b"file-loaded-secret\n").expect("write secret fixture");

    let cfg = cfg_with_secret_path(path.clone());
    let env = SecretEnv { value: None };
    let secret = auth::load_secret(&cfg, &env).expect("file path should load");
    assert_eq!(secret.len(), b"file-loaded-secret".len());
    assert!(secret.secret_id().starts_with("file:"));

    let token = BearerToken::from_bytes(b"file-loaded-secret".to_vec());
    assert!(bool::from(token.verify_against(&secret)));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn rejection_reason_carries_no_payload() {
    // Tatsu sanity: matching on the enum must not be able to bind any byte
    // slice or string from an inbound header. If a future edit adds a payload
    // variant, this match will stop compiling.
    use mutagen_service::auth::AuthRejectionReason as R;
    let reasons = [
        R::MissingHeader,
        R::MalformedHeader,
        R::UnknownScheme,
        R::SecretMismatch,
        R::NotConfigured,
    ];
    for r in reasons {
        let dbg = format!("{r:?}");
        assert!(
            !dbg.contains("bytes") && !dbg.contains("header_value"),
            "rejection variant should not have payload: {dbg}"
        );
    }
}
