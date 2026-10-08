//! Settings › Repositories: which local clone each GitHub repository maps to, and how
//! worktrees are managed.

use std::path::PathBuf;

use app_state_ui::StoresExt as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    notification::Notification,
    spinner::Spinner,
    switch::Switch,
};
use gpui_kit::{
    App, AppContext as _, Context, FontWeight, IntoElement, ParentElement as _, PathPromptOptions,
    Render, SharedString, Styled as _, Subscription, Window, div,
};
use pr_model::Timestamp;
use settings_model::{CloneStatus, Preferences, RepositoryMapping, display_path};
use theme_ui::TypographyExt as _;
use widgets_ui::{SettingsPage, SettingsRow, SettingsSection};

/// The Repositories page.
pub struct RepositoriesPage {
    _settings: Subscription,
}

impl RepositoriesPage {
    /// Creates the page.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let settings = cx.stores().settings().clone();
        Self {
            _settings: cx.observe(&settings, |_, _, cx| cx.notify()),
        }
    }
}

/// Asks for a folder, then hands it to `chosen`.
fn pick_folder(
    prompt: &str,
    window: &mut Window,
    cx: &mut App,
    chosen: impl FnOnce(PathBuf, &mut Window, &mut App) + 'static,
) {
    let paths = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some(prompt.to_string().into()),
    });
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            cx.update(|window, cx| chosen(path, window, cx)).ok();
        })
        .detach();
}

fn add_repository(window: &mut Window, cx: &mut App) {
    pick_folder("Add", window, cx, |path, window, cx| {
        let result = cx
            .stores()
            .settings()
            .clone()
            .update(cx, |settings, cx| settings.add_repository(&path, cx));
        match result {
            Ok(slug) => {
                window.push_notification(Notification::success(format!("Added {slug}.")), cx)
            }
            Err(error) => window.push_notification(
                Notification::error(format!("Couldn’t add {}: {error}", path.display())),
                cx,
            ),
        }
    });
}

fn status_line(
    mapping: &RepositoryMapping,
    status: &CloneStatus,
    now: Timestamp,
    home: Option<&std::path::Path>,
    cx: &App,
) -> impl IntoElement {
    let path = mapping
        .local_path
        .as_deref()
        .map(|path| display_path(path, home));
    let (icon, color, text): (Option<gpui_kit::AnyElement>, _, String) = match status {
        CloneStatus::Fetched { at, worktrees } => (
            Some(
                Icon::new(IconName::Check)
                    .xsmall()
                    .text_color(cx.theme().success)
                    .into_any_element(),
            ),
            cx.theme().muted_foreground,
            format!(
                "Fetched {} · {} {}",
                at.minutes_age(now),
                worktrees,
                if *worktrees == 1 {
                    "worktree"
                } else {
                    "worktrees"
                }
            ),
        ),
        CloneStatus::Fetching => (
            Some(
                Spinner::new()
                    .xsmall()
                    .color(cx.theme().warning)
                    .into_any_element(),
            ),
            cx.theme().muted_foreground,
            "Fetching…".into(),
        ),
        CloneStatus::NotCloned => (
            Some(Icon::new(IconName::Circle).xsmall().into_any_element()),
            cx.theme().muted_foreground,
            "Diffs come from the GitHub API until cloned".into(),
        ),
        CloneStatus::FolderNotFound => (
            Some(
                Icon::new(IconName::CircleAlert)
                    .xsmall()
                    .text_color(cx.theme().warning)
                    .into_any_element(),
            ),
            cx.theme().muted_foreground,
            "Folder not found".into(),
        ),
    };
    h_flex()
        .gap_1p5()
        .child(div().text_code(cx).child(SharedString::from(
            path.unwrap_or_else(|| "Not cloned".into()),
        )))
        .children(icon)
        .child(div().text_color(color).child(text))
}

fn preference_row(
    id: &'static str,
    title: &'static str,
    description: impl Into<SharedString>,
    checked: bool,
    update: fn(&mut Preferences, bool),
) -> impl IntoElement {
    SettingsRow::new(title)
        .description(description.into())
        .trailing(
            Switch::new(id)
                .checked(checked)
                .accessibility_label(title)
                .on_change(move |checked, _, cx| {
                    let checked = *checked;
                    cx.stores().settings().clone().update(cx, |settings, cx| {
                        settings.update_preferences(cx, |p| update(p, checked))
                    });
                }),
        )
}

impl Render for RepositoriesPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = cx.stores().settings().read(cx);
        let now = cx.stores().pull_requests().read(cx).now();
        let home = settings.home().map(|home| home.to_path_buf());
        let preferences = settings.preferences().clone();
        let mappings: Vec<(RepositoryMapping, CloneStatus)> = settings
            .repositories()
            .iter()
            .map(|mapping| (mapping.clone(), settings.clone_status(&mapping.slug)))
            .collect();

        let rows: Vec<_> = mappings
            .into_iter()
            .map(|(mapping, status)| {
                let slug = mapping.slug.clone();
                let action = match status {
                    CloneStatus::NotCloned => ("Clone…", "clone"),
                    CloneStatus::FolderNotFound => ("Locate…", "locate"),
                    _ => ("Change…", "change"),
                };
                let change_slug = slug.clone();
                let remove_slug = slug.clone();
                let reveal = mapping.local_path.clone();
                SettingsRow::with_title_element(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(SharedString::from(slug.clone())),
                )
                .leading(Icon::new(IconName::Github).text_color(cx.theme().muted_foreground))
                .description(status_line(&mapping, &status, now, home.as_deref(), cx))
                .trailing(
                    Button::new(SharedString::from(format!("{}-{slug}", action.1)))
                        .outline()
                        .small()
                        .label(action.0)
                        .on_click(move |_, window, cx| {
                            let slug = change_slug.clone();
                            if action.1 == "clone" {
                                pick_folder("Clone here", window, cx, move |parent, window, cx| {
                                    clone_into(slug.clone(), parent, window, cx)
                                });
                            } else {
                                pick_folder("Choose clone", window, cx, move |path, _, cx| {
                                    cx.stores().settings().clone().update(cx, |settings, cx| {
                                        settings.set_repository_path(&slug, path, cx)
                                    });
                                });
                            }
                        }),
                )
                .trailing(
                    Button::new(SharedString::from(format!("repo-more-{slug}")))
                        .ghost()
                        .small()
                        .icon(IconName::Ellipsis)
                        .accessibility_label("More repository actions")
                        .dropdown_menu_with_anchor(
                            gpui_kit::Anchor::TopRight,
                            move |menu: PopupMenu, _, _| {
                                let remove = remove_slug.clone();
                                let reveal = reveal.clone();
                                menu.item(
                                    PopupMenuItem::new("Reveal in Finder")
                                        .icon(Icon::new(IconName::Folder))
                                        .disabled(reveal.as_ref().is_none_or(|path| !path.is_dir()))
                                        .on_click(move |_, _, cx| {
                                            if let Some(path) = &reveal {
                                                cx.reveal_path(path);
                                            }
                                        }),
                                )
                                .separator()
                                .item(
                                    PopupMenuItem::new("Remove mapping").on_click(
                                        move |_, _, cx| {
                                            let slug = remove.clone();
                                            cx.stores().settings().clone().update(
                                                cx,
                                                |settings, cx| {
                                                    settings.remove_repository(&slug, cx)
                                                },
                                            );
                                        },
                                    ),
                                )
                            },
                        ),
                )
            })
            .collect();

        let worktree_root = home
            .as_deref()
            .map(|home| display_path(&home.join(".switchfold/worktrees"), Some(home)))
            .unwrap_or_else(|| "~/.switchfold/worktrees".into());

        SettingsPage::new(
            "repositories-page",
            "Repositories",
            "Map each GitHub repository to a local clone. Diffs, file views and agents read from the clone, so reviews match your working tree.",
        )
        .child(
            SettingsSection::new().rows(rows).after(
                Button::new("add-repository")
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("Add repository…")
                    .on_click(|_, window, cx| add_repository(window, cx)),
            ),
        )
        .child(
            SettingsSection::titled("Worktrees")
                .row(preference_row(
                    "fetch-on-open",
                    "Fetch when a pull request opens",
                    "Keeps diffs and CI status in sync with the remote.",
                    preferences.fetch_on_open,
                    |p, v| p.fetch_on_open = v,
                ))
                .row(preference_row(
                    "worktree-per-pr",
                    "Check out each pull request in its own worktree",
                    format!("Your current branch stays untouched. Worktrees live in {worktree_root}."),
                    preferences.worktree_per_pull_request,
                    |p, v| p.worktree_per_pull_request = v,
                ))
                .row(preference_row(
                    "remove-after-merge",
                    "Remove worktrees after merge",
                    "Deletes the worktree folder once the pull request is merged or closed.",
                    preferences.remove_worktree_after_merge,
                    |p, v| p.remove_worktree_after_merge = v,
                )),
        )
    }
}

fn clone_into(slug: String, parent: PathBuf, window: &mut Window, cx: &mut App) {
    let name = slug.rsplit('/').next().unwrap_or(&slug).to_string();
    let destination = parent.join(&name);
    let url = format!("https://github.com/{slug}.git");
    let target = destination.clone();
    let task = cx.background_spawn(async move {
        tool_path::command("git")
            .args(["clone", &url, &target.to_string_lossy()])
            .output()
    });
    window.push_notification(Notification::info(format!("Cloning {slug}…")), cx);
    window
        .spawn(cx, async move |cx| {
            let output = task.await;
            cx.update(|window, cx| match output {
                Ok(output) if output.status.success() => {
                    cx.stores().settings().clone().update(cx, |settings, cx| {
                        settings.set_repository_path(&slug, destination.clone(), cx)
                    });
                    window.push_notification(Notification::success(format!("Cloned {slug}.")), cx);
                }
                Ok(output) => {
                    let message = String::from_utf8_lossy(&output.stderr);
                    window.push_notification(
                        Notification::error(format!(
                            "Couldn’t clone {slug}: {}",
                            message.lines().last().unwrap_or("git failed")
                        )),
                        cx,
                    );
                }
                Err(error) => window.push_notification(
                    Notification::error(format!("Couldn’t run git: {error}")),
                    cx,
                ),
            })
            .ok();
        })
        .detach();
}
