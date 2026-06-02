//! Integration tests for the Project Registry — backs ISC-008, INV-P1, INV-P3,
//! DSD-623. The acceptance command is:
//!     cargo test -p mutagen-core --test project_registry

use mutagen_core::project::registry::{ProjectRegistry, ProjectStatus, RegisterError};
use mutagen_core::project::root::ProjectRootError;
use std::path::{Path, PathBuf};

fn tmp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nonce = format!(
        "mutagen-registry-{tag}-{pid}-{nanos}",
        pid = std::process::id(),
        nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    p.push(nonce);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn fresh_registry(tag: &str) -> (ProjectRegistry, PathBuf, PathBuf) {
    let svc = tmp_dir(tag);
    let workspace = {
        let mut w = svc.clone();
        w.push("workspace");
        std::fs::create_dir_all(&w).unwrap();
        w
    };
    let registry_path = svc.join("projects.toml");
    let registry = ProjectRegistry::load(&registry_path).expect("load empty registry");
    (registry, registry_path, workspace)
}

#[test]
fn register_absolute_path_succeeds() {
    let (mut reg, path, workspace) = fresh_registry("ok");
    let entry = reg.register("demo", &workspace).expect("register ok");

    assert_eq!(entry.name, "demo");
    assert_eq!(entry.status, ProjectStatus::Registered);
    assert!(!entry.project_id.is_empty(), "uuidv7 must be stamped");
    // DSD-623: uuidv7 is 36 chars with the version nibble pinned to 7.
    assert_eq!(entry.project_id.len(), 36);
    assert_eq!(&entry.project_id[14..15], "7");

    assert!(path.exists(), "registry file written");
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("project_id"), "toml shape preserved");

    // Re-loading sees the same entry — persistence round-trips.
    let reloaded = ProjectRegistry::load(&path).expect("reload");
    assert_eq!(reloaded.list().len(), 1);
    assert_eq!(reloaded.list()[0].project_id, entry.project_id);
}

#[test]
fn relative_path_rejected() {
    let (mut reg, _path, _ws) = fresh_registry("relrej");

    // A bare relative path — anything not starting with `/` or a drive letter.
    let result = reg.register("nope", Path::new("relative/path"));
    match result {
        Err(RegisterError::Root(ProjectRootError::Relative)) => {}
        other => panic!("expected Relative error, got {other:?}"),
    }
    assert!(reg.list().is_empty(), "no entry persisted on rejection");
}

#[test]
fn duplicate_canonical_root_rejected() {
    let (mut reg, _path, workspace) = fresh_registry("dup");
    let first = reg.register("one", &workspace).expect("first register ok");

    // Same root, different spelling — appending a `.` segment must canonicalize
    // to the same target and trip INV-P3.
    let aliased: PathBuf = workspace.join(".");
    let result = reg.register("two", &aliased);
    match result {
        Err(RegisterError::DuplicateRoot { existing_id }) => {
            assert_eq!(existing_id, first.project_id);
        }
        other => panic!("expected DuplicateRoot, got {other:?}"),
    }
    assert_eq!(reg.list().len(), 1, "duplicate must not be persisted");
}

#[test]
fn register_symlink_resolves_to_target() {
    let (mut reg, _path, workspace) = fresh_registry("symlink");

    // The target is the real workspace dir; the link is a sibling pointer at it.
    let parent = workspace.parent().unwrap();
    let link = parent.join("via_symlink");

    // Symlink creation is fallible on Windows without dev-mode or admin —
    // skip the test rather than red-fail in environments that can't.
    if !try_make_symlink(&workspace, &link) {
        eprintln!("skipping symlink test — host cannot create symlinks");
        return;
    }

    let first = reg.register("real", &workspace).expect("register target");
    let result = reg.register("via_link", &link);
    match result {
        Err(RegisterError::DuplicateRoot { existing_id }) => {
            assert_eq!(existing_id, first.project_id);
        }
        other => panic!("symlink should resolve to target — got {other:?}"),
    }
}

#[test]
fn archive_frees_root_for_reregistration() {
    let (mut reg, _path, workspace) = fresh_registry("archive");
    let first = reg.register("v1", &workspace).expect("register");
    reg.archive(&first.project_id).expect("archive");

    let second = reg
        .register("v2", &workspace)
        .expect("archive frees the root");
    assert_ne!(first.project_id, second.project_id);
    assert_eq!(reg.list().len(), 2);
}

#[test]
fn lookup_returns_registered_entry() {
    let (mut reg, _path, workspace) = fresh_registry("lookup");
    let entry = reg.register("look", &workspace).expect("register");
    let found = reg.lookup(&entry.project_id).expect("lookup hit");
    assert_eq!(found.name, "look");
    assert!(reg.lookup("not-a-real-id").is_err());
}

#[cfg(unix)]
fn try_make_symlink(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(windows)]
fn try_make_symlink(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_dir(target, link).is_ok()
}
