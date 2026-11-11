use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, tag::Tag};
use gpui_kit::{
    App, Hsla, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window,
    prelude::FluentBuilder as _,
};
use theme_ui::{ActiveAppTheme as _, TypographyExt as _};

/// The semantic color of a status tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Success,
    Danger,
    Warning,
    Merged,
    /// Agents and other automation.
    Info,
    /// Review requested and the viewer's own pending work.
    Accent,
}

impl Tone {
    /// The foreground color of this tone.
    pub fn color(self, cx: &App) -> Hsla {
        match self {
            Self::Neutral => cx.theme().muted_foreground,
            Self::Success => cx.theme().success,
            Self::Danger => cx.theme().danger,
            Self::Warning => cx.theme().warning,
            Self::Merged => cx.app_theme().merged,
            Self::Info => cx.theme().info,
            Self::Accent => cx.app_theme().accent,
        }
    }
}

/// A short tinted state label such as `Open`, `Merged` or `Expires soon`.
#[derive(IntoElement)]
pub struct StatusTag {
    tone: Tone,
    label: SharedString,
    icon: Option<IconName>,
}

impl StatusTag {
    /// A tag reading `label` in `tone`.
    pub fn new(tone: Tone, label: impl Into<SharedString>) -> Self {
        Self {
            tone,
            label: label.into(),
            icon: None,
        }
    }

    /// Adds a leading icon.
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
}

impl RenderOnce for StatusTag {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let foreground = self.tone.color(cx);
        let background = match self.tone {
            Tone::Neutral => cx.theme().muted,
            _ => foreground.opacity(0.15),
        };
        Tag::custom(background, foreground, background)
            .rounded(cx.theme().radius / 2.)
            .px_1p5()
            .py_0p5()
            .child(
                h_flex()
                    .gap_1()
                    .text_meta()
                    .when_some(self.icon, |this, icon| {
                        this.child(Icon::new(icon).xsmall().text_color(foreground))
                    })
                    .child(self.label),
            )
    }
}
