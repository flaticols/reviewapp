//! Finding command-line tools the way a login shell would. Apps launched from Finder
//! inherit a minimal `PATH`, so `gh`, `git` and `npx` installed by Homebrew, Nix or a
//! user profile would otherwise not be found.

use std::path::PathBuf;
use std::process::Command;

/// `PATH` plus Homebrew, Nix and per-user bin directories, in that order, without
/// duplicates.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let user = std::env::var("USER").unwrap_or_default();
    let mut extra = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/run/current-system/sw/bin"),
        PathBuf::from(format!("/etc/profiles/per-user/{user}/bin")),
        PathBuf::from("/nix/var/nix/profiles/default/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ];
    if let Some(home) = home {
        extra.push(home.join(".local/bin"));
        extra.push(home.join(".nix-profile/bin"));
        extra.push(home.join(".npm-global/bin"));
        extra.push(home.join(".bun/bin"));
        extra.push(home.join(".cargo/bin"));
    }
    for dir in extra {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// [`search_dirs`] joined for a child process's `PATH`.
pub fn path_env() -> String {
    search_dirs()
        .iter()
        .map(|dir| dir.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

/// Resolves `command` as a shell would: as a path when it contains `/`, otherwise by
/// searching [`search_dirs`].
pub fn find_on_path(command: &str) -> Option<PathBuf> {
    if command.contains('/') {
        let path = PathBuf::from(command);
        return path.is_file().then_some(path);
    }
    search_dirs()
        .into_iter()
        .map(|dir| dir.join(command))
        .find(|candidate| candidate.is_file())
}

/// A [`Command`] for the tool `name`, resolved through [`search_dirs`] and given the
/// same `PATH`, so tools it starts in turn are found too.
pub fn command(name: &str) -> Command {
    let mut command = Command::new(find_on_path(name).unwrap_or_else(|| PathBuf::from(name)));
    command.env("PATH", path_env());
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_absolute_and_bare_commands() {
        assert!(find_on_path("/bin/sh").is_some());
        assert!(find_on_path("/definitely/not/here").is_none());
        assert!(find_on_path("sh").is_some());
        assert!(
            command("sh")
                .arg("-c")
                .arg("exit 0")
                .status()
                .unwrap()
                .success()
        );
    }
}
