//! Settings › Triage: which provider rates findings, threads and pull requests, its
//! key, and which of those it does.

use app_state_ui::StoresExt as _;
use gpui_kit::component::{ActiveTheme as _, switch::Switch, v_flex};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Window, div,
};
use settings_model::Preferences;
use theme_ui::TypographyExt as _;
use triage_ui::TriageStoreExt as _;
use typesafe_ui::TypeSafeSection;
use widgets_ui::{SettingsPage, SettingsRow, SettingsSection};

/// The Triage page.
pub struct TriagePage {
    typesafe: Entity<TypeSafeSection>,
    _subscriptions: Vec<Subscription>,
}

impl TriagePage {
    /// Creates the page.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions =
            vec![cx.observe(&cx.stores().settings().clone(), |_, _, cx| cx.notify())];
        if let Some(store) = cx.triage() {
            subscriptions.push(cx.observe(&store, |_, _, cx| cx.notify()));
        }
        Self {
            typesafe: cx.new(|cx| TypeSafeSection::new(window, cx)),
            _subscriptions: subscriptions,
        }
    }
}

fn feature_row(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    checked: bool,
    update: fn(&mut Preferences, bool),
) -> impl IntoElement {
    SettingsRow::new(title).description(description).trailing(
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

impl Render for TriagePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let triage = cx.stores().settings().read(cx).preferences().triage.clone();
        let activity = cx.triage().map(|store| {
            let store = store.read(cx);
            match (store.has_provider(), store.last_error(), store.pending()) {
                (false, _, _) => "Off until the provider has a key.".to_string(),
                (true, Some(error), _) => format!("Paused after an error: {error}"),
                (true, None, 0) => "Up to date.".to_string(),
                (true, None, 1) => "Rating 1 item…".to_string(),
                (true, None, n) => format!("Rating {n} items…"),
            }
        });
        SettingsPage::new(
            "triage-page",
            "Triage",
            "A fast classifier rates agent findings, labels review threads and scores pull requests, so the important ones come first. Text from those goes to the provider with your key; answers are cached and only asked again when the text changes.",
        )
        .child(self.typesafe.clone())
        .child(
            SettingsSection::titled("What it rates")
                .row(feature_row(
                    "triage-findings",
                    "Agent findings",
                    "Severity, category, and whether a finding is worth acting on. Likely noise is dimmed.",
                    triage.findings,
                    |p, v| p.triage.findings = v,
                ))
                .row(feature_row(
                    "triage-threads",
                    "Review threads",
                    "Blocking, suggestion, nit, question or praise, and whether it looks addressed.",
                    triage.threads,
                    |p, v| p.triage.threads = v,
                ))
                .row(feature_row(
                    "triage-pull-requests",
                    "Pull requests in the inbox",
                    "Kind and risk from the title, description, size and changed paths.",
                    triage.pull_requests,
                    |p, v| p.triage.pull_requests = v,
                )),
        )
        .children(activity.map(|activity| {
            v_flex().child(
                div()
                    .text_meta()
                    .text_color(cx.theme().muted_foreground)
                    .child(activity),
            )
        }))
    }
}
