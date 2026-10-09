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

/// How many alerts and details may nest inside each other. Each level is a text view
/// inside the last, and the source is anyone's comment, so nesting is counted where it
/// happens, while rendering, not guessed from the text.
pub(crate) const MAX_NESTING: usize = 8;

/// A text view of already prepared Markdown, with Switchfold's block plugins.
pub(crate) fn view(id: impl Into<ElementId>, prepared: impl Into<SharedString>) -> TextView {
    view_at(id, prepared, 0)
}

/// [`view`] for the body of an alert or details block `nesting` levels deep. Past
/// [`MAX_NESTING`], alerts and details render as the plain quotes and code blocks they
/// are written as.
pub(crate) fn view_at(
    id: impl Into<ElementId>,
    prepared: impl Into<SharedString>,
    nesting: usize,
) -> TextView {
    TextView::markdown(id, prepared)
        .plugin(alerts::AlertPlugin { nesting })
        .plugin(mermaid::MermaidPlugin)
        .plugin(details::DetailsPlugin { nesting })
}
