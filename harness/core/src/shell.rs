use std::process::Command;

#[cfg(windows)]
use std::path::{Path, PathBuf};

/// Bash on Windows is a minefield. A bare `bash` lookup grabs whatever lands
/// first on PATH, and on GitHub's windows-latest runner that's the System32 WSL
/// stub — which, with no distro installed, just exits 1 and drags every command
/// that shells out down with it. Git for Windows ships the bash we actually
/// want, so go hunt that down first and only fall back to a bare `bash` when
/// it's nowhere to be found (notably every non-Windows host, where bare `bash`
/// is exactly right).
pub fn bash_command() -> Command {
    Command::new(resolve_bash())
}

#[cfg(not(windows))]
fn resolve_bash() -> std::ffi::OsString {
    std::ffi::OsString::from("bash")
}

#[cfg(windows)]
fn resolve_bash() -> std::ffi::OsString {
    git_bash_path()
        .map(PathBuf::into_os_string)
        .unwrap_or_else(|| std::ffi::OsString::from("bash"))
}

#[cfg(windows)]
fn git_bash_path() -> Option<PathBuf> {
    let mut git_roots: Vec<PathBuf> = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(var) {
            git_roots.push(PathBuf::from(dir).join("Git"));
        }
    }
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        git_roots.push(PathBuf::from(dir).join("Programs").join("Git"));
    }
    if let Some(root) = git_root_from_path() {
        git_roots.push(root);
    }

    git_roots.into_iter().find_map(|root| {
        ["bin\\bash.exe", "usr\\bin\\bash.exe"]
            .iter()
            .map(|leaf| root.join(leaf))
            .find(|candidate| candidate.is_file())
    })
}

/// Belt-and-suspenders for installs that don't sit under a standard Program
/// Files dir: walk PATH to find git.exe, then climb back up to the Git root that
/// has a bash next to it.
#[cfg(windows)]
fn git_root_from_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if !dir.join("git.exe").is_file() {
            continue;
        }
        // git.exe lives in <root>\cmd or <root>\mingw64\bin depending on the
        // install, so just climb parents until one looks like the Git root.
        let mut ancestor: Option<&Path> = Some(dir.as_path());
        while let Some(current) = ancestor {
            if current.join("usr\\bin\\bash.exe").is_file()
                || current.join("bin\\bash.exe").is_file()
            {
                return Some(current.to_path_buf());
            }
            ancestor = current.parent();
        }
    }
    None
}
