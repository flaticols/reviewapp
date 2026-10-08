//! The pull request list and each pull request's review data, kept on disk so the app
//! shows the last known state right away and refreshes from GitHub behind it.
//!
//! One directory per account holds `list.json` and `pulls/<owner>/<name>/<number>.json`,
//! each wrapped with a format version and when it was saved. Files from another version
//! read as missing.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use pr_model::{PullRequest, PullRequestKey, Timestamp};
use review_model::PullRequestDetail;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Bumped whenever a cached type changes shape, so older files are ignored rather than
/// misread.
const FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Envelope<T> {
    format: u32,
    saved_at: Timestamp,
    data: T,
}

/// Something read back from the cache, with when it was saved.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Cached<T> {
    pub data: T,
    pub saved_at: Timestamp,
}

/// One account's cache directory.
#[derive(Clone, Debug)]
pub struct PullRequestCache {
    dir: PathBuf,
}

impl PullRequestCache {
    /// The cache for `login` on `host` under `root`, such as
    /// `~/Library/Caches/dev.switchfold.review`.
    pub fn new(root: &Path, host: &str, login: &str) -> Self {
        Self {
            dir: root.join(sanitize(host)).join(sanitize(login)),
        }
    }

    /// The macOS cache root for the app under `home`.
    pub fn default_root(home: &Path) -> PathBuf {
        home.join("Library/Caches/dev.switchfold.review")
    }

    /// The account's cache directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The pull request list as last saved.
    pub fn load_list(&self) -> Option<Cached<Vec<PullRequest>>> {
        read(&self.list_path())
    }

    /// Saves the pull request list.
    pub fn save_list(&self, pull_requests: &[PullRequest], saved_at: Timestamp) -> io::Result<()> {
        write(&self.list_path(), pull_requests, saved_at)
    }

    /// The review data for `key` as last saved.
    pub fn load_detail(&self, key: &PullRequestKey) -> Option<Cached<PullRequestDetail>> {
        read(&self.detail_path(key))
    }

    /// Saves the review data for `key`.
    pub fn save_detail(
        &self,
        key: &PullRequestKey,
        detail: &PullRequestDetail,
        saved_at: Timestamp,
    ) -> io::Result<()> {
        write(&self.detail_path(key), detail, saved_at)
    }

    /// Deletes review data not saved within `max_age`, and returns how many files went.
    pub fn prune(&self, max_age: Duration) -> usize {
        let Some(cutoff) = SystemTime::now().checked_sub(max_age) else {
            return 0;
        };
        let mut removed = 0;
        let mut stack = vec![self.dir.join("pulls")];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                if meta.is_dir() {
                    stack.push(path);
                } else if meta.modified().is_ok_and(|modified| modified < cutoff)
                    && fs::remove_file(&path).is_ok()
                {
                    removed += 1;
                }
            }
        }
        removed
    }

    /// Deletes everything cached for the account.
    pub fn clear(&self) -> io::Result<()> {
        match fs::remove_dir_all(&self.dir) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    fn list_path(&self) -> PathBuf {
        self.dir.join("list.json")
    }

    fn detail_path(&self, key: &PullRequestKey) -> PathBuf {
        self.dir
            .join("pulls")
            .join(sanitize(key.repo().owner()))
            .join(sanitize(key.repo().name()))
            .join(format!("{}.json", key.number()))
    }
}

/// Keeps a path component to characters GitHub logins, hosts and repository names use,
/// so no value can climb out of the cache directory.
fn sanitize(component: &str) -> String {
    let clean: String = component
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() || clean.chars().all(|c| c == '.') {
        "_".into()
    } else {
        clean
    }
}

fn read<T: DeserializeOwned>(path: &Path) -> Option<Cached<T>> {
    let bytes = fs::read(path).ok()?;
    let envelope: Envelope<T> = serde_json::from_slice(&bytes).ok()?;
    (envelope.format == FORMAT).then_some(Cached {
        data: envelope.data,
        saved_at: envelope.saved_at,
    })
}

/// Writes through a temporary file and a rename, so a crash mid-write leaves the old
/// copy rather than a truncated one.
fn write<T: Serialize + ?Sized>(path: &Path, data: &T, saved_at: Timestamp) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let envelope = Envelope {
        format: FORMAT,
        saved_at,
        data,
    };
    let bytes = serde_json::to_vec(&envelope).map_err(io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pr_model::RepoSlug;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pr-cache-{name}-{}-{}",
            std::process::id(),
            Timestamp::now().unix()
        ));
        fs::remove_dir_all(&dir).ok();
        dir
    }

    #[test]
    fn details_round_trip_per_account() {
        let root = scratch("round-trip");
        let key = PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482);
        let summary = PullRequest::new(
            key.clone(),
            "Title",
            "octocat",
            "feature",
            "main",
            Timestamp::from_unix(10),
        );
        let detail = PullRequestDetail::new(summary, "Body", Timestamp::from_unix(10));
        let mine = PullRequestCache::new(&root, "github.com", "denis");
        mine.save_detail(&key, &detail, Timestamp::from_unix(20))
            .unwrap();
        let loaded = mine.load_detail(&key).expect("saved detail reads back");
        assert_eq!(loaded.data.body, "Body");
        assert_eq!(loaded.saved_at, Timestamp::from_unix(20));
        let theirs = PullRequestCache::new(&root, "github.com", "someone-else");
        assert!(theirs.load_detail(&key).is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn another_format_reads_as_missing() {
        let root = scratch("format");
        let cache = PullRequestCache::new(&root, "github.com", "denis");
        cache.save_list(&[], Timestamp::from_unix(1)).unwrap();
        assert!(cache.load_list().is_some());
        fs::write(
            cache.list_path(),
            r#"{"format":999,"saved_at":1,"data":[]}"#,
        )
        .unwrap();
        assert!(cache.load_list().is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn components_cannot_escape_the_directory() {
        assert_eq!(sanitize("../.."), ".._..");
        assert_eq!(sanitize(".."), "_");
        assert_eq!(sanitize("a/b"), "a_b");
        assert_eq!(sanitize(""), "_");
    }
}
