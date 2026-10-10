use app_state_ui::StoresExt as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    notification::Notification,
    select::{Select, SelectEvent, SelectState},
    spinner::Spinner,
};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, Window, div, prelude::FluentBuilder as _, rems,
};
use theme_ui::TypographyExt as _;
use typesafe::{Client, DEFAULT_MODEL};
use widgets_ui::{EditMenu, SettingsRow, SettingsSection, StatusTag, Tone};

use crate::{KEYS_URL, PROVIDER_ID};

/// The models offered before a key check lists what the account may use.
const KNOWN_MODELS: [&str; 2] = [DEFAULT_MODEL, "jev-preview"];

/// Where the last key check got to.
enum Check {
    Idle,
    Running,
    Works(Vec<String>),
    Failed(String),
}

/// Settings for TypeSafe.ai: the API key, kept in the Keychain, and the model.
pub struct TypeSafeSection {
    key: Entity<InputState>,
    model: Entity<SelectState<Vec<SharedString>>>,
    check: Check,
    _subscriptions: Vec<Subscription>,
}

impl TypeSafeSection {
    /// Creates the section.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Paste a TypeSafe.ai API key")
                .context_menu(false)
        });
        let current = cx
            .stores()
            .settings()
            .read(cx)
            .preferences()
            .typesafe
            .model
            .clone();
        let model = cx.new(|cx| {
            let options = Self::model_options(&current, &[]);
            let selected = options.iter().position(|o| o.as_ref() == current);
            SelectState::new(options, selected.map(IndexPath::new), window, cx)
        });
        let subscriptions = vec![cx.subscribe_in(
            &model,
            window,
            |_, _, event: &SelectEvent<Vec<SharedString>>, _, cx| {
                let SelectEvent::Confirm(Some(choice)) = event else {
                    return;
                };
                let choice = choice.to_string();
                cx.stores().settings().clone().update(cx, |settings, cx| {
                    settings.update_preferences(cx, |p| p.typesafe.model = choice)
                });
            },
        )];
        Self {
            key,
            model,
            check: Check::Idle,
            _subscriptions: subscriptions,
        }
    }

    fn model_options(current: &str, listed: &[String]) -> Vec<SharedString> {
        let mut options: Vec<String> = KNOWN_MODELS.iter().map(|m| m.to_string()).collect();
        for name in listed.iter().chain(std::iter::once(&current.to_string())) {
            if !name.is_empty() && !options.contains(name) {
                options.push(name.clone());
            }
        }
        options.into_iter().map(SharedString::from).collect()
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.key.read(cx).value().trim().to_string();
        if key.is_empty() {
            return;
        }
        let stored = keychain::store_api_key(PROVIDER_ID, &key);
        match stored {
            Ok(()) => {
                self.key
                    .update(cx, |input, cx| input.set_value("", window, cx));
                crate::set_saved_key(Some(key), cx);
                self.run_check(cx);
            }
            Err(error) => window.push_notification(Notification::error(error.to_string()), cx),
        }
        cx.notify();
    }

    fn remove(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match keychain::delete_api_key(PROVIDER_ID) {
            Ok(()) => {
                crate::set_saved_key(None, cx);
                self.check = Check::Idle;
            }
            Err(error) => window.push_notification(Notification::error(error.to_string()), cx),
        }
        cx.notify();
    }

    fn run_check(&mut self, cx: &mut Context<Self>) {
        let Some(key) = crate::saved_key(cx).map(str::to_string) else {
            return;
        };
        let model = cx
            .stores()
            .settings()
            .read(cx)
            .preferences()
            .typesafe
            .model
            .clone();
        self.check = Check::Running;
        let task = cx.background_spawn(async move { Client::new(key, model).models() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.check = match result {
                    Ok(models) => Check::Works(models.into_iter().map(|m| m.name).collect()),
                    Err(error) => Check::Failed(error.to_string()),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn key_hint(key: &str) -> String {
        let tail: String = key
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("Saved in the Keychain · ends in {tail}")
    }
}

impl Render for TypeSafeSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = crate::saved_key(cx).map(Self::key_hint);
        let has_key = saved.is_some();
        let status: gpui_kit::AnyElement = match &self.check {
            Check::Idle => div()
                .text_color(cx.theme().muted_foreground)
                .child(if has_key {
                    "Not checked yet"
                } else {
                    "No key yet"
                })
                .into_any_element(),
            Check::Running => h_flex()
                .gap_1p5()
                .child(Spinner::new().xsmall())
                .child("Checking the key…")
                .into_any_element(),
            Check::Works(models) => {
                StatusTag::new(Tone::Success, format!("Key works · {}", models.join(", ")))
                    .icon(IconName::CircleCheck)
                    .into_any_element()
            }
            Check::Failed(error) => div()
                .text_color(cx.theme().danger)
                .child(error.clone())
                .into_any_element(),
        };
        SettingsSection::titled("TypeSafe.ai")
            .row(
                SettingsRow::new("API key")
                    .description(SharedString::from(match &saved {
                        Some(hint) => hint.clone(),
                        None => "Bring your own key. It stays in the macOS Keychain and only goes to api.typesafe.ai.".to_string(),
                    }))
                    .trailing(
                        h_flex()
                            .gap_1()
                            .child(
                                EditMenu::new(
                                    "typesafe-key-edit-menu",
                                    &self.key,
                                    Input::new(&self.key).small().mask_toggle(),
                                )
                                .w(rems(15.)),
                            )
                            .child(
                                Button::new("typesafe-save-key")
                                    .small()
                                    .primary()
                                    .label(if has_key { "Replace" } else { "Save" })
                                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                            )
                            .when(has_key, |this| {
                                this.child(
                                    Button::new("typesafe-remove-key")
                                        .small()
                                        .ghost()
                                        .label("Remove")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.remove(window, cx)
                                        })),
                                )
                            }),
                    ),
            )
            .row(
                SettingsRow::new("Model")
                    .description("jev-latest follows TypeSafe’s newest stable model.")
                    .trailing(div().w(rems(11.25)).child(Select::new(&self.model).small())),
            )
            .row(
                SettingsRow::new("Status")
                    .description(div().text_ui().child(status))
                    .trailing(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("typesafe-check")
                                    .small()
                                    .outline()
                                    .label("Check key")
                                    .disabled(!has_key || matches!(self.check, Check::Running))
                                    .on_click(cx.listener(|this, _, _, cx| this.run_check(cx))),
                            )
                            .child(
                                Button::new("typesafe-get-key")
                                    .small()
                                    .ghost()
                                    .icon(IconName::ExternalLink)
                                    .label("Get a key")
                                    .on_click(|_, _, cx| cx.open_url(KEYS_URL)),
                            ),
                    ),
            )
    }
}
