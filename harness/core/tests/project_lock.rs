//! Integration tests for the Project Lock primitive.
//! Backs ISC-004 (single writer per project) and ISC-005 (per-command).

use mutagen_core::project::lock::{
    AcquireError, EXIT_LOCK_HELD, LockHolder, acquire, holder_path, read_record,
};
use std::process;
use std::sync::{Arc, Barrier};
use std::thread;

fn tmp_root(tag: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let nonce = format!(
        "mutagen-lock-{tag}-{pid}-{nanos}",
        tag = tag,
        pid = process::id(),
        nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    p.push(nonce);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn acquire_writes_identity_and_releases_on_drop() {
    let root = tmp_root("ident");
    let holder = LockHolder::Cli { pid: process::id() };
    {
        let guard = acquire(&root, holder.clone()).expect("acquire");
        guard.assert_owned().expect("self-owned");
        let rec = read_record(&root).expect("record present");
        assert_eq!(rec.holder, holder);
        assert!(!rec.acquired_at.is_empty());
    }
    let again = acquire(&root, holder).expect("re-acquire after drop");
    drop(again);
}

#[test]
fn two_writers_one_succeeds_one_fails_78() {
    let root = tmp_root("rivals");
    let barrier = Arc::new(Barrier::new(2));

    let r1 = root.clone();
    let b1 = barrier.clone();
    let h1 = thread::spawn(move || {
        b1.wait();
        acquire(&r1, LockHolder::Cli { pid: 11111 })
    });

    let r2 = root.clone();
    let b2 = barrier.clone();
    let h2 = thread::spawn(move || {
        b2.wait();
        acquire(&r2, LockHolder::Cli { pid: 22222 })
    });

    let res1 = h1.join().unwrap();
    let res2 = h2.join().unwrap();

    let (winner, loser) = match (res1, res2) {
        (Ok(g), Err(e)) => (g, e),
        (Err(e), Ok(g)) => (g, e),
        (Ok(_), Ok(_)) => panic!("both writers acquired the lock"),
        (Err(_), Err(_)) => panic!("both writers failed"),
    };

    let _winner = winner;
    match loser {
        AcquireError::Held(_) | AcquireError::HeldAnonymous => {}
        AcquireError::Io(e) => panic!("expected Held, got io: {e:#}"),
    }
    assert_eq!(EXIT_LOCK_HELD, 78);
}

#[test]
fn stale_holder_pid_dead_break_allowed() {
    let root = tmp_root("stale");
    // Stage a holder file from a prior, dead process. fs2 OS locks are
    // released on process exit, so the practical "stale" residue is an
    // identity file orphaned from a holder that's no longer running.
    // u32::MAX-1 is never assigned by Linux/Windows PID allocators.
    let dead_pid = u32::MAX - 1;
    std::fs::create_dir_all(root.join(".mutagen/state")).unwrap();
    std::fs::write(
        holder_path(&root),
        format!("cli:{dead_pid} 2026-01-01T00:00:00Z\n"),
    )
    .unwrap();

    let g = acquire(&root, LockHolder::Cli { pid: process::id() })
        .expect("stale residue must not prevent acquisition");
    let rec = read_record(&root).expect("our record");
    assert!(matches!(rec.holder, LockHolder::Cli { pid } if pid == process::id()));
    drop(g);
}

#[test]
fn live_holder_break_forbidden() {
    let root = tmp_root("live");
    let me = process::id();
    let g = acquire(&root, LockHolder::Cli { pid: me }).expect("first acquire");

    let r2 = root.clone();
    let attempted = thread::spawn(move || acquire(&r2, LockHolder::Cli { pid: 33333 }))
        .join()
        .unwrap();

    match attempted {
        Err(AcquireError::Held(rec)) => {
            assert!(matches!(rec.holder, LockHolder::Cli { pid } if pid == me));
        }
        Err(AcquireError::HeldAnonymous) => {
            // Acceptable: peeking thread raced past identity write.
        }
        Ok(_) => panic!("second acquire should have failed against live holder"),
        Err(AcquireError::Io(e)) => panic!("unexpected io error: {e:#}"),
    }

    drop(g);
}

#[test]
fn release_then_reacquire_is_clean() {
    let root = tmp_root("recycle");
    let h = LockHolder::Service {
        session_id: "sess-1".into(),
    };
    let g = acquire(&root, h.clone()).unwrap();
    g.release().unwrap();
    let g2 = acquire(&root, h).unwrap();
    g2.assert_owned().unwrap();
}
