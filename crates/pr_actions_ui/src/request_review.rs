use app_state_ui::StoresExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::DialogFooter,
    h_flex,
    input::{Input, InputEvent, InputState},
    notification::Notification,
    spinner::Spinner,
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, rems,
};
use pr_model::PullRequestKey;
use review_model::Actor;
use theme_ui::TypographyExt as _;
use widgets_ui::{ActorName, EditMenu, InitialsAvatar};

/// The Request review dialog's state: everyone who can review, filtered by a search,
/// minus the author and people already asked.
struct RequestReviewForm {
    key: PullRequestKey,
    query: Entity<InputState>,
    people: Option<Result<Vec<Actor>, String>>,
    excluded: Vec<String>,
    selected: Vec<Actor>,
}

impl RequestReviewForm {
    fn new(key: PullRequestKey, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search people")
                .context_menu(false)
        });
        cx.subscribe(&query, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        let store = cx.stores().pull_requests().clone();
        let mut excluded: Vec<String> = Vec::new();
        if let Some(pr) = store.read(cx).pull_request(&key) {
            excluded.push(pr.author.clone());
        }
        if let Some(detail) = store.read(cx).loaded_detail(&key) {
            excluded.push(detail.summary.author.clone());
            excluded.extend(detail.reviewers.iter().map(|r| r.actor.login.clone()));
        }
        let task = store.update(cx, |store, cx| store.assignable_users(key.repo(), cx));
        cx.spawn(async move |this, cx| {
            let people = task.await.map_err(|error| error.to_string());
            this.update(cx, |this, cx| {
                this.people = Some(people);
                cx.notify();
            })
            .ok();
        })
        .detach();
        query.update(cx, |query, cx| query.focus(window, cx));
        Self {
            key,
            query,
            people: None,
            excluded,
            selected: Vec::new(),
        }
    }

    fn toggle(&mut self, actor: Actor, cx: &mut Context<Self>) {
        match self.selected.iter().position(|a| a.login == actor.login) {
            Some(ix) => {
                self.selected.remove(ix);
            }
            None => self.selected.push(actor),
        }
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            return;
        }
        let key = self.key.clone();
        let reviewers = self.selected.clone();
        let names: Vec<String> = reviewers
            .iter()
            .map(|a| a.display_name().to_string())
            .collect();
        let store = cx.stores().pull_requests().clone();
        let task = store.update(cx, |store, cx| store.request_reviewers(&key, reviewers, cx));
        window
            .spawn(cx, async move |cx| {
                let result = task.await;
                cx.update(|window, cx| match result {
                    Ok(()) => window.push_notification(
                        Notification::success(format!(
                            "Asked {} to review #{}.",
                            names.join(", "),
                            key.number()
                        )),
                        cx,
                    ),
                    Err(error) => {
                        window.push_notification(Notification::error(error.to_string()), cx)
                    }
                })
                .ok();
            })
            .detach();
        window.close_dialog(cx);
    }
}

impl Render for RequestReviewForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.query.read(cx).value().to_lowercase();
        let body = match &self.people {
            None => h_flex()
                .h_10()
                .gap_2()
                .text_ui()
                .text_color(cx.theme().muted_foreground)
                .child(Spinner::new().small())
                .child("Loading people who can review…")
                .into_any_element(),
            Some(Err(error)) => div()
                .text_ui()
                .text_color(cx.theme().danger)
                .child(format!("Couldn’t load reviewers: {error}"))
                .into_any_element(),
            Some(Ok(people)) => {
                let matches: Vec<Actor> = people
                    .iter()
                    .filter(|actor| !self.excluded.contains(&actor.login))
                    .filter(|actor| {
                        query.is_empty()
                            || actor.login.to_lowercase().contains(&query)
                            || actor
                                .name
                                .as_deref()
                                .is_some_and(|name| name.to_lowercase().contains(&query))
                    })
                    .cloned()
                    .collect();
                if matches.is_empty() {
                    div()
                        .text_ui()
                        .text_color(cx.theme().muted_foreground)
                        .child("Nobody matches.")
                        .into_any_element()
                } else {
                    v_flex()
                        .id("reviewer-candidates")
                        .max_h(rems(18.))
                        .overflow_y_scroll()
                        .gap_0p5()
                        .children(matches.into_iter().map(|actor| {
                            let checked = self.selected.iter().any(|a| a.login == actor.login);
                            let toggle = actor.clone();
                            h_flex()
                                .gap_2()
                                .h_8()
                                .text_ui()
                                .child(
                                    Checkbox::new(SharedString::from(format!(
                                        "reviewer-{}",
                                        actor.login
                                    )))
                                    .checked(checked)
                                    .on_change(cx.listener(move |this, _: &bool, _, cx| {
                                        this.toggle(toggle.clone(), cx)
                                    })),
                                )
                                .child(InitialsAvatar::new(actor.display_name()))
                                .child(ActorName::new(&actor))
                        }))
                        .into_any_element()
                }
            }
        };
        v_flex()
            .gap_3()
            .child(EditMenu::new(
                "query-edit-menu",
                &self.query,
                Input::new(&self.query),
            ))
            .child(body)
    }
}

/// Opens the Request review dialog for `key`.
pub fn open_request_review(key: PullRequestKey, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| RequestReviewForm::new(key.clone(), window, cx));
    let width = window.rem_size() * 26.;
    window.open_dialog(cx, move |dialog, _, cx| {
        let submit_form = form.clone();
        let count = form.read(cx).selected.len();
        dialog
            .title(format!("Request review on #{}", key.number()))
            .w(width)
            .child(form.clone())
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("request-review-cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("request-review-submit")
                            .primary()
                            .label(match count {
                                0 | 1 => "Request review".to_string(),
                                n => format!("Request {n} reviews"),
                            })
                            .disabled(count == 0)
                            .on_click(move |_, window, cx| {
                                submit_form.update(cx, |form, cx| form.submit(window, cx))
                            }),
                    ),
            )
    });
}
