use acp_client::{Decision, PermissionPrompt};
use app_state_ui::StoresExt as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, FontWeight, IntoElement as _, ParentElement as _, Styled as _, div,
    prelude::FluentBuilder as _,
};
use theme_ui::TypographyExt as _;

/// Answers `prompt` without asking when the reviewer turned asking off, always allows
/// its command in `repository`, or lets agents run commands that only read and this one
/// does. Returns the prompt when the reviewer must decide.
pub(crate) fn screen(
    prompt: PermissionPrompt,
    repository: &str,
    cx: &App,
) -> Option<PermissionPrompt> {
    let preferences = cx.stores().settings().read(cx).preferences();
    let remembered = prompt
        .command()
        .is_some_and(|command| preferences.is_command_allowed(repository, command));
    let reads = preferences.allow_read_only_commands
        && prompt.command().is_some_and(crate::read_only::is_read_only);
    if !preferences.ask_before_commands || remembered || reads {
        prompt.respond(if remembered {
            Decision::AllowAlways
        } else {
            Decision::AllowOnce
        });
        return None;
    }
    Some(prompt)
}

/// Sends the reviewer's `decision`. `AllowAlways` also remembers the command for
/// `repository`.
pub(crate) fn answer(prompt: PermissionPrompt, decision: Decision, repository: &str, cx: &mut App) {
    if decision == Decision::AllowAlways
        && let Some(command) = prompt.command().map(str::to_string)
    {
        let repository = repository.to_string();
        cx.stores().settings().clone().update(cx, |settings, cx| {
            settings.update_preferences(cx, |preferences| {
                preferences
                    .always_allowed_commands
                    .push((repository, command))
            })
        });
    }
    prompt.respond(decision);
}

/// The agent's request to run something, with Allow once, Always allow and Deny.
pub(crate) fn prompt_card(
    prompt: &PermissionPrompt,
    agent: &str,
    cwd: &str,
    on_answer: impl Fn(Decision, &mut App) + Clone + 'static,
    cx: &App,
) -> AnyElement {
    let command = prompt.command().map(str::to_string);
    let heading = match &command {
        Some(_) => format!("{agent} wants to run a command in {cwd}"),
        None => format!("{agent} asks to {}", prompt.title().to_lowercase()),
    };
    let (allow_once, allow_always, deny) = (on_answer.clone(), on_answer.clone(), on_answer);
    v_flex()
        .gap_2()
        .p_3()
        .rounded(cx.theme().radius)
        .bg(cx.theme().warning.opacity(0.12))
        .border_1()
        .border_color(cx.theme().warning.opacity(0.35))
        .child(
            h_flex()
                .gap_2()
                .text_ui()
                .font_weight(FontWeight::MEDIUM)
                .child(
                    Icon::new(IconName::CircleAlert)
                        .small()
                        .text_color(cx.theme().warning),
                )
                .child(heading),
        )
        .when_some(command, |this, command| {
            this.child(
                div()
                    .px_2()
                    .py_1p5()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().background.opacity(0.6))
                    .text_code(cx)
                    .child(format!("$ {command}")),
            )
        })
        .child(
            h_flex()
                .gap_1()
                .child(
                    Button::new("allow-once")
                        .small()
                        .icon(IconName::Check)
                        .label("Allow once")
                        .on_click(move |_, _, cx| allow_once(Decision::AllowOnce, cx)),
                )
                .when(prompt.offers(Decision::AllowAlways), |this| {
                    this.child(
                        Button::new("allow-always")
                            .ghost()
                            .small()
                            .label("Always allow in this repo")
                            .on_click(move |_, _, cx| allow_always(Decision::AllowAlways, cx)),
                    )
                })
                .child(div().flex_1())
                .child(
                    Button::new("deny")
                        .ghost()
                        .small()
                        .icon(IconName::X)
                        .label("Deny")
                        .on_click(move |_, _, cx| deny(Decision::Deny, cx)),
                ),
        )
        .into_any_element()
}
