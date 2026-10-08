use std::path::PathBuf;

use pr_model::{PullRequestKey, RepoSlug};

use crate::percent;

/// The URL scheme Switchfold registers.
pub const SCHEME: &str = "dev.switchfold.review";

/// A `dev.switchfold.review://` link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwitchfoldLink {
    /// `comment?file=<absolute path>&line=<n>`: start a comment on that line of the pull
    /// request the file's checkout belongs to.
    Comment { file: PathBuf, line: u32 },
    /// `open?pr=<owner/name#n>[&file=<path>][&line=<n>]`: show a pull request, optionally
    /// at a file relative to the repository root.
    Open {
        key: PullRequestKey,
        file: Option<String>,
        line: Option<u32>,
    },
}

impl SwitchfoldLink {
    /// Parses a link; `None` for another scheme or an unknown or incomplete action.
    pub fn parse(url: &str) -> Option<Self> {
        let rest = url.strip_prefix(SCHEME)?.strip_prefix("://")?;
        let (action, query) = rest.split_once('?').unwrap_or((rest, ""));
        let param = |name: &str| {
            query.split('&').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                (key == name).then(|| percent::decode(value))
            })
        };
        let line = param("line").and_then(|line| line.trim().parse::<u32>().ok());
        match action.trim_end_matches('/') {
            "comment" => Some(Self::Comment {
                file: PathBuf::from(param("file")?),
                line: line?.max(1),
            }),
            "open" => {
                let pr = param("pr")?;
                let (repo, number) = pr.split_once('#')?;
                Some(Self::Open {
                    key: PullRequestKey::new(RepoSlug::parse(repo)?, number.parse().ok()?),
                    file: param("file").filter(|file| !file.is_empty()),
                    line,
                })
            }
            _ => None,
        }
    }

    /// The link as a URL.
    pub fn to_url(&self) -> String {
        match self {
            Self::Comment { file, line } => format!(
                "{SCHEME}://comment?file={}&line={line}",
                percent::encode(&file.to_string_lossy(), false)
            ),
            Self::Open { key, file, line } => {
                let mut url = format!(
                    "{SCHEME}://open?pr={}",
                    percent::encode(&format!("{}#{}", key.repo(), key.number()), false)
                );
                if let Some(file) = file {
                    url.push_str(&format!("&file={}", percent::encode(file, false)));
                }
                if let Some(line) = line {
                    url.push_str(&format!("&line={line}"));
                }
                url
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_links_round_trip() {
        let link = SwitchfoldLink::Comment {
            file: PathBuf::from("/Users/me/Code Review/a&b.rs"),
            line: 42,
        };
        assert_eq!(SwitchfoldLink::parse(&link.to_url()), Some(link));
    }

    #[test]
    fn open_links_name_the_pull_request() {
        let link = SwitchfoldLink::Open {
            key: PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482),
            file: Some("src/lib.rs".into()),
            line: Some(7),
        };
        let url = link.to_url();
        assert!(url.starts_with("dev.switchfold.review://open?pr=flaticols%2Fswitchfold%23482"));
        assert_eq!(SwitchfoldLink::parse(&url), Some(link));
    }

    #[test]
    fn rejects_other_links() {
        assert_eq!(SwitchfoldLink::parse("zed://file/a"), None);
        assert_eq!(
            SwitchfoldLink::parse("dev.switchfold.review://comment?file=%2Fa"),
            None
        );
        assert_eq!(
            SwitchfoldLink::parse("dev.switchfold.review://delete"),
            None
        );
    }
}
