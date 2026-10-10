//! The timeline's building blocks: comment cards, event rows, commit rows, agent
//! finding cards and check rows.

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClipboardItem, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _,
};
use pr_model::{CheckStatus, RepoSlug};
use review_model::{Actor, CheckRun, Commit};
use theme_ui::TypographyExt as _;
use widgets_ui::{ActorName, AvatarSize, CheckIcon, InitialsAvatar, StatusTag, Tone};

/// The avatar for an actor: initials for people, a bot glyph for agents.
pub fn actor_avatar(actor: &Actor, size: AvatarSize) -> InitialsAvatar {
    if actor.is_agent {
        InitialsAvatar::icon(IconName::Bot).size(size)
    } else {
        InitialsAvatar::new(actor.display_name()).size(size)
    }
}

/// A card holding one comment: author, verb and age, the markdown body, and actions.
#[derive(IntoElement)]
pub struct CommentCard {
    id: SharedString,
    author: Actor,
    verb: SharedString,
    body: SharedString,
    is_pending: bool,
    badges: Vec<AnyElement>,
    repo: Option<RepoSlug>,
    actions: Vec<AnyElement>,
}

impl CommentCard {
    /// A card for `body` by `author`; `verb` reads like `commented 2h ago`.
    pub fn new(
        id: impl Into<SharedString>,
        author: Actor,
        verb: impl Into<SharedString>,
        body: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            author,
            verb: verb.into(),
            body: body.into(),
            is_pending: false,
            badges: Vec::new(),
            repo: None,
            actions: Vec::new(),
        }
    }

    /// Marks the comment as part of the viewer's pending review.
    pub fn pending(mut self, is_pending: bool) -> Self {
        self.is_pending = is_pending;
        self
    }

    /// Adds a tag after the author line, such as a triage label.
    pub fn badge(mut self, badge: impl IntoElement) -> Self {
        self.badges.push(badge.into_any_element());
        self
    }

    /// Links `@login` and `#123` in the body the way GitHub does in `repo`.
    pub fn repo(mut self, repo: RepoSlug) -> Self {
        self.repo = Some(repo);
        self
    }

    /// Adds an action under the body, such as Reply or Resolve.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl RenderOnce for CommentCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let body = self.body.clone();
        v_flex()
            .gap_2()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .gap_2()
                    .child(actor_avatar(&self.author, AvatarSize::Medium))
                    .child(div().text_ui().child(ActorName::new(&self.author)))
                    .child(
                        div()
                            .text_ui()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.verb),
                    )
                    .when(self.is_pending, |this| {
                        this.child(StatusTag::new(Tone::Accent, "Pending"))
                    })
                    .children(self.badges)
                    .child(div().flex_1())
                    .child(
                        Button::new(SharedString::from(format!("{}-more", self.id)))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Ellipsis)
                            .accessibility_label("Comment actions")
                            .dropdown_menu_with_anchor(
                                gpui_kit::Anchor::TopRight,
                                move |menu: PopupMenu, _, _| {
                                    let body = body.clone();
                                    menu.item(
                                        PopupMenuItem::new("Copy text")
                                            .icon(Icon::new(IconName::Copy))
                                            .on_click(move |_, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    body.to_string(),
                                                ))
                                            }),
                                    )
                                },
                            ),
                    ),
            )
            .child(
                div().pl_8().text_body().child(
                    markdown_ui::github_markdown(self.id.clone(), &self.body, self.repo.as_ref())
                        .selectable(true),
                ),
            )
            .when(!self.actions.is_empty(), |this| {
                this.child(h_flex().pl_6().gap_1().children(self.actions))
            })
    }
}

/// A one-line event such as `denis pushed 2 commits`, with its age at the trailing edge.
#[derive(IntoElement)]
pub struct EventRow {
    avatar: AnyElement,
    actor: Actor,
    text: SharedString,
    age: Option<SharedString>,
}

impl EventRow {
    /// An event by `actor` described by `text`.
    pub fn new(avatar: impl IntoElement, actor: &Actor, text: impl Into<SharedString>) -> Self {
        Self {
            avatar: avatar.into_any_element(),
            actor: actor.clone(),
            text: text.into(),
            age: None,
        }
    }

    /// The event's age, such as `2d ago`.
    pub fn age(mut self, age: impl Into<SharedString>) -> Self {
        self.age = Some(age.into());
        self
    }
}

impl RenderOnce for EventRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .h_8()
            .px_3()
            .gap_2()
            .text_ui()
            .child(self.avatar)
            .child(ActorName::new(&self.actor))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.text),
            )
            .when_some(self.age, |this, age| {
                this.child(div().text_color(cx.theme().muted_foreground).child(age))
            })
    }
}

/// A commit under a push event: message, short hash and CI status.
#[derive(IntoElement)]
pub struct CommitRow {
    commit: Commit,
}

impl CommitRow {
    /// A row for `commit`.
    pub fn new(commit: Commit) -> Self {
        Self { commit }
    }
}

impl RenderOnce for CommitRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .h_8()
            .pl_12()
            .pr_3()
            .gap_2()
            .text_ui()
            .child(
                Icon::new(IconName::GitCommitHorizontal)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(SharedString::from(self.commit.message.clone())),
            )
            .child(
                div()
                    .text_code(cx)
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(self.commit.short_sha().to_string())),
            )
            .when_some(self.commit.ci, |this, ci| this.child(CheckIcon::new(ci)))
    }
}

/// A check run as a timeline row, with its elapsed time and a Details button.
#[derive(IntoElement)]
pub struct CheckRow {
    check: CheckRun,
    details: Option<AnyElement>,
}

impl CheckRow {
    /// A row for `check`.
    pub fn new(check: CheckRun) -> Self {
        Self {
            check,
            details: None,
        }
    }

    /// The button that opens the log.
    pub fn details(mut self, button: impl IntoElement) -> Self {
        self.details = Some(button.into_any_element());
        self
    }
}

impl RenderOnce for CheckRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let sha = &self.check.head_sha[..self.check.head_sha.len().min(7)];
        let state = match self.check.status {
            CheckStatus::Passed => "passed on",
            CheckStatus::Failed => "failed on",
            CheckStatus::Running => "running on",
            CheckStatus::Queued => "queued on",
            CheckStatus::Skipped => "skipped on",
        };
        h_flex()
            .h_8()
            .px_3()
            .gap_2()
            .text_ui()
            .child(CheckIcon::new(self.check.status))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(format!("{} · {state} {sha}", self.check.name)),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(pr_model::format_duration(self.check.elapsed_secs)),
            )
            .when_some(self.details, |this, details| this.child(details))
    }
}
