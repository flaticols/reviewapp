use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex};
use gpui_kit::{
    App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Window, prelude::FluentBuilder as _, rems,
};
use review_model::initials;

/// The three avatar sizes the design uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AvatarSize {
    /// 20 px: inline with timeline and inspector text.
    #[default]
    Small,
    /// 24 px: comment headers.
    Medium,
    /// 32 px: account rows.
    Large,
}

/// A neutral circle with a person's initials, or an icon for agents and events.
///
/// gpui-kit's `Avatar` tints initials by identity; the design keeps every avatar
/// neutral so status colors stay meaningful.
#[derive(IntoElement)]
pub struct InitialsAvatar {
    initials: SharedString,
    icon: Option<IconName>,
    size: AvatarSize,
}

impl InitialsAvatar {
    /// An avatar showing the initials of `name`.
    pub fn new(name: &str) -> Self {
        Self {
            initials: initials(name).into(),
            icon: None,
            size: AvatarSize::Small,
        }
    }

    /// An avatar showing an icon instead of initials.
    pub fn icon(icon: IconName) -> Self {
        Self {
            initials: SharedString::default(),
            icon: Some(icon),
            size: AvatarSize::Small,
        }
    }

    /// Sets the size.
    pub fn size(mut self, size: AvatarSize) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for InitialsAvatar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (diameter, text) = match self.size {
            AvatarSize::Small => (rems(1.25), rems(0.5625)),
            AvatarSize::Medium => (rems(1.5), rems(0.625)),
            AvatarSize::Large => (rems(2.), rems(0.75)),
        };
        h_flex()
            .flex_shrink_0()
            .justify_center()
            .size(diameter)
            .rounded_full()
            .bg(cx.theme().muted)
            .text_color(cx.theme().muted_foreground)
            .text_size(text)
            .font_weight(FontWeight::SEMIBOLD)
            .map(|this| match self.icon {
                Some(icon) => this.child(Icon::new(icon).xsmall()),
                None => this.child(self.initials),
            })
    }
}
