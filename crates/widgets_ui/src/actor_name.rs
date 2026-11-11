use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Window, div, prelude::FluentBuilder as _,
};
use review_model::Actor;

/// Who did something, as rows and comment headers show it: the full name in semibold
/// and the `@handle` muted beside it. People without a profile name, agents and teams
/// show their one name.
#[derive(IntoElement)]
pub struct ActorName {
    actor: Actor,
}

impl ActorName {
    /// The name of `actor`.
    pub fn new(actor: &Actor) -> Self {
        Self {
            actor: actor.clone(),
        }
    }
}

impl RenderOnce for ActorName {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let handle = (!self.actor.is_agent && self.actor.name.is_some())
            .then(|| SharedString::from(format!("@{}", self.actor.login)));
        h_flex()
            .flex_shrink_0()
            .gap_1()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(self.actor.display_name().to_string())),
            )
            .when_some(handle, |this, handle| {
                this.child(div().text_color(cx.theme().muted_foreground).child(handle))
            })
    }
}
