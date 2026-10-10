use serde::Serialize;
use sha2::{Digest as _, Sha256};

/// What a provider sees of an agent finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct FindingInput {
    pub file: String,
    pub line: u32,
    /// The finding as the agent wrote it, in Markdown.
    pub finding: String,
    /// Lines of the diff around `line`, each prefixed `+`, `-` or a space.
    pub code: Vec<String>,
}

/// One comment in a review thread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Comment {
    pub author: String,
    pub text: String,
}

/// What a provider sees of a review thread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ThreadInput {
    pub file: String,
    pub line: u32,
    /// Oldest first.
    pub comments: Vec<Comment>,
    pub code: Vec<String>,
}

/// What a provider sees of a pull request in the inbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct PullRequestInput {
    pub title: String,
    /// The start of the description.
    pub description: String,
    pub head_branch: String,
    pub base_branch: String,
    pub labels: Vec<String>,
    pub changed_files: u32,
    pub additions: u32,
    pub deletions: u32,
    /// Changed paths, as many as the list query fetched.
    pub paths: Vec<String>,
}

impl FindingInput {
    /// A finding on `line` of `file`.
    pub fn new(
        file: impl Into<String>,
        line: u32,
        finding: impl Into<String>,
        code: Vec<String>,
    ) -> Self {
        Self {
            file: file.into(),
            line,
            finding: finding.into(),
            code,
        }
    }
}

impl Comment {
    /// `text` by `author`.
    pub fn new(author: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            author: author.into(),
            text: text.into(),
        }
    }
}

impl ThreadInput {
    /// A thread on `line` of `file`.
    pub fn new(
        file: impl Into<String>,
        line: u32,
        comments: Vec<Comment>,
        code: Vec<String>,
    ) -> Self {
        Self {
            file: file.into(),
            line,
            comments,
            code,
        }
    }
}

impl PullRequestInput {
    /// A pull request titled `title`; fill the other public fields directly.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: String::new(),
            head_branch: String::new(),
            base_branch: String::new(),
            labels: Vec::new(),
            changed_files: 0,
            additions: 0,
            deletions: 0,
            paths: Vec::new(),
        }
    }
}

/// A hex digest of `input`, which changes whenever anything a provider would see does.
pub fn fingerprint(input: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(input).unwrap_or_default();
    Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

macro_rules! fingerprinted {
    ($($ty:ty),*) => {$(
        impl $ty {
            /// A digest of everything a provider sees, for caching answers.
            pub fn fingerprint(&self) -> String {
                fingerprint(self)
            }
        }
    )*};
}

fingerprinted!(FindingInput, ThreadInput, PullRequestInput);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_follow_the_content() {
        let a = FindingInput::new("a.rs", 3, "Off by one", vec!["+x".into()]);
        let mut b = a.clone();
        assert_eq!(a.fingerprint(), b.fingerprint());
        b.finding.push('!');
        assert_ne!(a.fingerprint(), b.fingerprint());
    }
}
