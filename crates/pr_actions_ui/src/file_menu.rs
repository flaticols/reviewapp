use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon,
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::{ClipboardItem, SharedString};
use pr_model::RepoSlug;
use sha2::{Digest as _, Sha256};

/// A file of a pull request, for its context menu.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct FileTarget {
    pub repo: RepoSlug,
    pub number: u64,
    /// The head commit, for permalinks; links fall back to the branch view without it.
    pub head_sha: Option<String>,
    /// Path relative to the repository root.
    pub path: String,
}

impl FileTarget {
    /// The file at `path` in pull request `number` of `repo`.
    pub fn new(
        repo: RepoSlug,
        number: u64,
        head_sha: Option<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            repo,
            number,
            head_sha,
            path: path.into(),
        }
    }

    /// The file at the head commit, which keeps pointing at the same lines after new
    /// pushes.
    pub fn permalink(&self) -> Option<String> {
        self.head_sha
            .as_ref()
            .map(|sha| format!("https://github.com/{}/blob/{sha}/{}", self.repo, self.path))
    }

    /// The file's diff in the pull request's Files changed tab. GitHub anchors each file
    /// by the SHA-256 of its path.
    pub fn diff_url(&self) -> String {
        let anchor: String = Sha256::digest(self.path.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!(
            "https://github.com/{}/pull/{}/files#diff-{anchor}",
            self.repo, self.number
        )
    }

    /// A link to one line of the file. Head-side lines link the file at the head commit,
    /// which GitHub shows as a code snippet in comments; a base-side line only exists in
    /// the diff, so it links the diff's left column there.
    pub fn line_url(&self, line: u32, base: bool) -> Option<String> {
        if base {
            return Some(format!("{}L{line}", self.diff_url()));
        }
        self.permalink().map(|link| format!("{link}#L{line}"))
    }
}

/// Adds the GitHub actions for `target` to `menu`: copy its path or permalink, open it
/// on GitHub, open its diff on GitHub. Callers add their own local actions after these.
pub fn file_menu(menu: PopupMenu, target: &FileTarget) -> PopupMenu {
    let path = target.path.clone();
    let permalink = target.permalink();
    let diff = target.diff_url();
    let menu = menu.item(
        PopupMenuItem::new("Copy path")
            .icon(Icon::new(IconName::Copy))
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(path.clone()))
            }),
    );
    let menu = match permalink.clone() {
        Some(link) => menu.item(
            PopupMenuItem::new("Copy permalink")
                .icon(Icon::new(IconName::Link))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(link.clone()))
                }),
        ),
        None => menu,
    };
    let menu = match permalink {
        Some(link) => menu.item(
            PopupMenuItem::new("Open file on GitHub")
                .icon(Icon::new(IconName::Globe))
                .on_click(move |_, _, cx| cx.open_url(&link)),
        ),
        None => menu,
    };
    menu.item(
        PopupMenuItem::new(SharedString::from("Open diff on GitHub"))
            .icon(Icon::new(IconName::ExternalLink))
            .on_click(move |_, _, cx| cx.open_url(&diff)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_pin_the_head_commit_and_anchor_the_diff() {
        let file = FileTarget::new(
            RepoSlug::new("octocat", "hello-world"),
            42,
            Some("abc1234".into()),
            "internal/api/a.go",
        );
        assert_eq!(
            file.permalink().as_deref(),
            Some("https://github.com/octocat/hello-world/blob/abc1234/internal/api/a.go")
        );
        let diff = file.diff_url();
        assert!(diff.starts_with("https://github.com/octocat/hello-world/pull/42/files#diff-"));
        assert_eq!(diff.rsplit('-').next().map(str::len), Some(64));
    }

    #[test]
    fn line_links_pick_the_blob_for_head_lines_and_the_diff_for_base_lines() {
        let file = FileTarget::new(
            RepoSlug::new("octocat", "hello-world"),
            42,
            Some("abc1234".into()),
            "a.go",
        );
        assert_eq!(
            file.line_url(12, false).as_deref(),
            Some("https://github.com/octocat/hello-world/blob/abc1234/a.go#L12")
        );
        assert_eq!(
            file.line_url(7, true),
            Some(format!("{}L7", file.diff_url()))
        );
        let unpinned = FileTarget::new(RepoSlug::new("o", "r"), 1, None, "a.go");
        assert_eq!(unpinned.line_url(12, false), None);
    }
}
