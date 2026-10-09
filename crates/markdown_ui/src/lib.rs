//! Markdown as GitHub renders it, for descriptions, comments and agent replies.
//!
//! On top of what gpui-kit's text view parses (tables, task lists, strikethrough,
//! autolinks, footnotes, images), this adds GitHub's alerts (`> [!NOTE]` and the rest),
//! Mermaid diagrams, collapsible `<details>`, `<kbd>`, `<sub>`, `<sup>`, emoji
//! shortcodes, hidden HTML comments, and `@login` and `#123` links.

mod alerts;
mod details;
mod mermaid;
mod source;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use gpui_kit::component::text::TextView;
use gpui_kit::{ElementId, SharedString};
use pr_model::RepoSlug;

/// How many prepared documents [`github_markdown`] keeps, so a render doesn't redo the
/// source pass for text it has seen.
const PREPARED_CACHE: usize = 512;

/// Prepared Markdown by source text and repository.
type PreparedCache = HashMap<(String, Option<String>), SharedString>;

static PREPARED: LazyLock<Mutex<PreparedCache>> = LazyLock::new(Mutex::default);

/// A text view of `source` as GitHub shows it. With `repo`, `@login` and `#123` link to
/// GitHub the way they do there.
pub fn github_markdown(
    id: impl Into<ElementId>,
    source: &str,
    repo: Option<&RepoSlug>,
) -> TextView {
    let key = (source.to_string(), repo.map(ToString::to_string));
    let prepared = {
        let mut cache = PREPARED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(prepared) = cache.get(&key) {
            prepared.clone()
        } else {
            if cache.len() >= PREPARED_CACHE {
                cache.clear();
            }
            let prepared = SharedString::from(source::prepare(source, repo));
            cache.insert(key, prepared.clone());
            prepared
        }
    };
    view(id, prepared)
}

/// A text view of already prepared Markdown, with Switchfold's block plugins. Alerts and
/// details render their bodies through this too.
pub(crate) fn view(id: impl Into<ElementId>, prepared: impl Into<SharedString>) -> TextView {
    TextView::markdown(id, prepared)
        .plugin(alerts::AlertPlugin)
        .plugin(mermaid::MermaidPlugin)
        .plugin(details::DetailsPlugin)
}
