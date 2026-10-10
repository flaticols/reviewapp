use gpui_kit::assets::IconName;
use std::collections::HashSet;

use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    list::ListItem,
    tree::{TreeItem, tree},
    v_flex,
};
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, div,
    prelude::FluentBuilder as _, relative, rems,
};
use review_model::{FindingState, PullRequestDetail};
use theme_ui::TypographyExt as _;
use widgets_ui::{Counter, SectionHeader, StatusTag};

use crate::{ReviewThreadEvent, ReviewThreadView, filters};

impl ReviewThreadView {
    pub(crate) fn render_outline(
        &mut self,
        detail: &PullRequestDetail,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let viewed = detail
            .files
            .iter()
            .filter(|f| detail.is_viewed(&f.path))
            .count();
        let unresolved = filters::unresolved_count(detail);

        let view = cx.entity().downgrade();
        let menu_view = view.clone();
        let files = tree(&self.file_tree, move |_, entry, _, _, cx| {
            let depth = entry.depth() as f32;
            let row = ListItem::new(SharedString::from(format!("outline-{}", entry.item().id)))
                .h(rems(1.625))
                .pl(rems(0.5 + depth * 0.875))
                .pr_2()
                .rounded(cx.theme().radius)
                .accessibility_label(entry.item().id.clone());
            if entry.is_folder() {
                return row.child(
                    h_flex()
                        .gap_1p5()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            Icon::new(if entry.is_expanded() {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .xsmall(),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_ui()
                                .child(entry.item().label.clone()),
                        ),
                );
            }
            let path = entry.item().id.clone();
            let (is_viewed, additions, deletions) = view
                .upgrade()
                .and_then(|view| {
                    let view = view.read(cx);
                    let detail = view.detail.as_ref()?;
                    // The tree lists the whole pull request, also while one commit is
                    // picked, so its counts come from the pull request's files.
                    let file = detail
                        .files
                        .iter()
                        .find(|file| file.path == path.as_ref())?;
                    Some((detail.is_viewed(&path), file.additions(), file.deletions()))
                })
                .unwrap_or_default();
            row.child(
                h_flex()
                    .gap_2()
                    .child(
                        Icon::new(if is_viewed {
                            IconName::SquareCheck
                        } else {
                            IconName::FileCode
                        })
                        .small()
                        .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_ui()
                            .when(is_viewed, |this| {
                                this.text_color(cx.theme().muted_foreground)
                            })
                            .child(entry.item().label.clone()),
                    )
                    .child(
                        div()
                            .text_meta()
                            .text_color(cx.theme().success)
                            .child(format!("+{additions}")),
                    )
                    .child(
                        div()
                            .text_meta()
                            .text_color(cx.theme().danger)
                            .child(format!("−{deletions}")),
                    ),
            )
        })
        .context_menu(move |_, entry, menu, _, cx| match menu_view.upgrade() {
            Some(view) if !entry.is_folder() => {
                ReviewThreadView::file_menu(&view, &entry.item().id, menu, cx)
            }
            _ => menu,
        });

        let mut thread_rows: Vec<ThreadRow> = detail
            .unresolved_threads()
            .map(|thread| {
                let starter = thread
                    .starter()
                    .map(|actor| actor.display_name().to_string())
                    .unwrap_or_default();
                let label = crate::triage::thread(&self.key, &thread.id, cx);
                ThreadRow {
                    id: thread.id.clone().into(),
                    icon: IconName::MessageSquare,
                    label: format!("{starter} · {}", thread.anchor_label()),
                    count: thread.comments.len(),
                    path: thread.path.clone(),
                    rank: crate::triage::thread_rank(label),
                    tag: label
                        .filter(|label| label.kind == triage::ThreadKind::Blocking)
                        .map(|label| crate::triage::thread_tag(&label)),
                }
            })
            .collect();
        thread_rows.extend(
            detail
                .findings
                .iter()
                .filter(|finding| finding.state == FindingState::Draft)
                .map(|finding| {
                    let rated = crate::triage::finding(&self.key, &finding.id, cx);
                    ThreadRow {
                        id: finding.id.clone().into(),
                        icon: IconName::Bot,
                        label: format!("{} · {}", finding.agent.login, finding.anchor_label()),
                        count: 1,
                        path: finding.path.clone(),
                        rank: crate::triage::finding_rank(rated),
                        tag: rated
                            .filter(|rated| rated.severity >= triage::Severity::Major)
                            .map(|rated| {
                                StatusTag::new(widgets_ui::Tone::Danger, rated.severity.label())
                            }),
                    }
                }),
        );
        thread_rows.sort_by(|a, b| b.label.cmp(&a.label));
        thread_rows.sort_by_key(|row| detail.files.iter().position(|f| f.path == row.path));
        // Blocking threads and severe findings first; file order within each rank.
        thread_rows.sort_by_key(|row| row.rank);

        let threads = thread_rows.into_iter().map(
            |ThreadRow {
                 id,
                 icon,
                 label,
                 count,
                 path,
                 tag,
                 ..
             }| {
                ListItem::new(SharedString::from(format!("outline-thread-{id}")))
                    .h_7()
                    .px_2()
                    .rounded(cx.theme().radius)
                    .on_click(cx.listener(move |this, _, _, cx| this.reveal_file(&path, cx)))
                    .child(
                        h_flex()
                            .gap_2()
                            .text_ui()
                            .child(
                                Icon::new(icon)
                                    .small()
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .child(div().flex_1().min_w_0().truncate().child(label))
                            .children(tag)
                            .child(Counter::new(count)),
                    )
            },
        );

        v_flex()
            .id("outline")
            .test_support()
            .size_full()
            .h_full()
            .p_2()
            .gap_0p5()
            .bg(cx.theme().sidebar)
            .child(
                h_flex().child(
                    Button::new("back")
                        .ghost()
                        .small()
                        .icon(IconName::ArrowLeft)
                        .label(self.back_label.clone())
                        .tooltip("Back to pull requests")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ReviewThreadEvent::Back))),
                ),
            )
            .child(div().h_2())
            .child(SectionHeader::new(format!(
                "Files · {viewed} of {} viewed",
                detail.files.len()
            )))
            .child(div().flex_1().min_h_0().child(files))
            .child(div().h_3())
            .child(SectionHeader::new(format!(
                "Threads · {unresolved} unresolved"
            )))
            .child(
                v_flex()
                    .id("outline-threads")
                    .flex_shrink_0()
                    .max_h(relative(0.35))
                    .overflow_y_scroll()
                    .gap_0p5()
                    .children(threads),
            )
    }

    /// Rebuilds the file tree when the set of changed files differs from what it shows,
    /// keeping folders the reviewer folded.
    pub(crate) fn sync_file_tree(&mut self, detail: &PullRequestDetail, cx: &mut Context<Self>) {
        let paths: Vec<String> = detail.files.iter().map(|file| file.path.clone()).collect();
        if paths == self.file_tree_paths {
            return;
        }
        let folded = self.folded_dirs.clone();
        let items = to_tree_items(&build_file_tree(&paths), &folded);
        self.file_tree
            .update(cx, |tree, cx| tree.set_items(items, cx));
        self.file_tree_paths = paths;
    }

    /// Reveals the file the tree selected, by click or arrow keys, once per selection.
    pub(crate) fn on_file_tree_changed(&mut self, cx: &mut Context<Self>) {
        let selected = self
            .file_tree
            .read(cx)
            .selected_entry()
            .filter(|entry| !entry.is_folder())
            .map(|entry| entry.item().id.to_string());
        if let Some(path) = selected
            && self.tree_revealed.as_ref() != Some(&path)
        {
            self.reveal_file(&path, cx);
            self.tree_revealed = Some(path);
        }
    }
}

/// A folder in the changed-file tree. Chains of folders with a single subfolder and no
/// files are merged into one, named like `crates/review/src`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct FileTreeDir {
    pub name: String,
    /// The folder's path, used as its id.
    pub path: String,
    pub dirs: Vec<FileTreeDir>,
    /// Full paths of the files directly inside.
    pub files: Vec<String>,
}

/// Arranges `paths` into folders, keeping their order within each folder.
pub(crate) fn build_file_tree(paths: &[String]) -> FileTreeDir {
    let mut root = FileTreeDir::default();
    for full in paths {
        let mut node = &mut root;
        let mut parts: Vec<&str> = full.split('/').collect();
        parts.pop();
        let mut prefix = String::new();
        for part in parts {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            let ix = match node.dirs.iter().position(|dir| dir.name == part) {
                Some(ix) => ix,
                None => {
                    node.dirs.push(FileTreeDir {
                        name: part.to_string(),
                        path: prefix.clone(),
                        ..FileTreeDir::default()
                    });
                    node.dirs.len() - 1
                }
            };
            node = &mut node.dirs[ix];
        }
        node.files.push(full.clone());
    }
    for dir in &mut root.dirs {
        compact(dir);
    }
    root
}

fn compact(dir: &mut FileTreeDir) {
    while dir.files.is_empty() && dir.dirs.len() == 1 {
        let child = dir.dirs.remove(0);
        dir.name = format!("{}/{}", dir.name, child.name);
        dir.path = child.path;
        dir.dirs = child.dirs;
        dir.files = child.files;
    }
    for child in &mut dir.dirs {
        compact(child);
    }
}

fn to_tree_items(dir: &FileTreeDir, folded: &HashSet<String>) -> Vec<TreeItem> {
    let dirs = dir.dirs.iter().map(|child| {
        TreeItem::new(format!("{}/", child.path), child.name.clone())
            .expanded(!folded.contains(&child.path))
            .children(to_tree_items(child, folded))
    });
    let files = dir.files.iter().map(|path| {
        let name = path.rsplit('/').next().unwrap_or(path);
        TreeItem::new(path.clone(), name.to_string())
    });
    dirs.chain(files).collect()
}

/// One open thread or draft finding in the sidebar list.
struct ThreadRow {
    id: SharedString,
    icon: IconName,
    label: String,
    count: usize,
    path: String,
    /// Lower sorts first; see [`crate::triage::thread_rank`].
    rank: u8,
    tag: Option<StatusTag>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_single_folder_chains_and_keeps_order() {
        let paths: Vec<String> = [
            "crates/review/src/thread.rs",
            "crates/review/src/hunk.rs",
            "crates/diff/src/lib.rs",
            "Cargo.toml",
        ]
        .map(String::from)
        .to_vec();
        let root = build_file_tree(&paths);
        assert_eq!(root.files, vec!["Cargo.toml"]);
        assert_eq!(root.dirs.len(), 1);
        let crates = &root.dirs[0];
        assert_eq!(crates.name, "crates");
        let names: Vec<&str> = crates.dirs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["review/src", "diff/src"]);
        assert_eq!(crates.dirs[0].path, "crates/review/src");
        assert_eq!(
            crates.dirs[0].files,
            ["crates/review/src/thread.rs", "crates/review/src/hunk.rs"]
        );
    }
}
