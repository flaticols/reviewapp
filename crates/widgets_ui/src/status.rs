use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, spinner::Spinner};
use gpui_kit::{AnyElement, App, Hsla, IntoElement, RenderOnce, Styled as _, Window};
use pr_model::{CheckStatus, PullRequestState};
use theme_ui::ActiveAppTheme as _;

/// The color that communicates `status`, alongside its icon shape.
pub fn check_color(status: CheckStatus, cx: &App) -> Hsla {
    match status {
        CheckStatus::Passed => cx.theme().success,
        CheckStatus::Failed => cx.theme().danger,
        CheckStatus::Running => cx.theme().warning,
        CheckStatus::Queued | CheckStatus::Skipped => cx.theme().muted_foreground,
    }
}

/// A 16 px icon for a CI status: check, cross, spinning loader, or empty circle. The
/// shape differs per status so color is never the only cue.
#[derive(IntoElement)]
pub struct CheckIcon {
    status: CheckStatus,
}

impl CheckIcon {
    /// The icon for `status`.
    pub fn new(status: CheckStatus) -> Self {
        Self { status }
    }
}

impl RenderOnce for CheckIcon {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = check_color(self.status, cx);
        let element: AnyElement = match self.status {
            CheckStatus::Passed => Icon::new(IconName::CircleCheck)
                .text_color(color)
                .into_any_element(),
            CheckStatus::Failed => Icon::new(IconName::CircleX)
                .text_color(color)
                .into_any_element(),
            CheckStatus::Running => Spinner::new()
                .icon(Icon::new(IconName::LoaderCircle))
                .color(color)
                .into_any_element(),
            CheckStatus::Queued | CheckStatus::Skipped => Icon::new(IconName::Circle)
                .text_color(color)
                .into_any_element(),
        };
        element
    }
}

/// The pull request glyph for a lifecycle state, colored by that state.
#[derive(IntoElement)]
pub struct PullRequestStateIcon {
    state: PullRequestState,
    small: bool,
}

impl PullRequestStateIcon {
    /// The icon for `state`.
    pub fn new(state: PullRequestState) -> Self {
        Self {
            state,
            small: false,
        }
    }

    /// Renders at 14 px instead of 16 px.
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }
}

impl RenderOnce for PullRequestStateIcon {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (icon, color) = match self.state {
            PullRequestState::Open => (IconName::GitPullRequest, cx.theme().success),
            PullRequestState::Draft => (IconName::GitPullRequestDraft, cx.theme().muted_foreground),
            PullRequestState::Merged => (IconName::GitMerge, cx.app_theme().merged),
            PullRequestState::Closed => (IconName::GitPullRequestClosed, cx.theme().danger),
        };
        let icon = Icon::new(icon).text_color(color);
        if self.small { icon.small() } else { icon }
    }
}
