use acp_client::{Decision, PlanStatus, RunOutcome, ToolKind, ToolStatus};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    progress::Progress,
    spinner::Spinner,
    text::TextView,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Entity, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _,
};
use pr_model::format_duration;
use theme_ui::{ActiveAppTheme as _, TypographyExt as _};
use widgets_ui::{AvatarSize, InitialsAvatar};

use crate::{Activity, AgentRun, RunState};

/// Activity entries the full view lists, newest last.
const ACTIVITY_SHOWN: usize = 12;

/// The thread card for an agent review in progress: progress, what the agent is doing
/// now, the permission prompt when it asks to run something, and Stop. The Reviews tab
/// shows the same run in full with [`AgentRunDetails`].
#[derive(IntoElement)]
pub struct AgentRunCard {
    run: Entity<AgentRun>,
}

impl AgentRunCard {
    /// A card for `run`.
    pub fn new(run: Entity<AgentRun>) -> Self {
        Self { run }
    }
}

impl RenderOnce for AgentRunCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let run = self.run.read(cx);
        let now = current_step(run);
        card(cx)
            .child(header(&self.run, cx))
            .child(progress_bar(run, cx))
            .when_some(now, |this, (icon, text)| {
                this.child(
                    h_flex()
                        .gap_2()
                        .pl_8()
                        .text_ui()
                        .text_color(cx.theme().muted_foreground)
                        .child(icon)
                        .child(div().flex_1().min_w_0().truncate().child(text)),
                )
            })
            .children(permission(&self.run, cx))
            .children(failure(&self.run, cx))
    }
}

/// An agent review in full, for the Reviews tab: plan, activity log, the agent's reply
/// as it streams, and the permission prompt.
#[derive(IntoElement)]
pub struct AgentRunDetails {
    run: Entity<AgentRun>,
}

impl AgentRunDetails {
    /// The full view of `run`.
    pub fn new(run: Entity<AgentRun>) -> Self {
        Self { run }
    }
}

impl RenderOnce for AgentRunDetails {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let run = self.run.read(cx);
        let id = self.run.entity_id().as_u64();
        let (seen, total) = run.progress();
        let plan = run.plan().to_vec();
        let activity: Vec<Activity> = run
            .activity()
            .iter()
            .rev()
            .take(ACTIVITY_SHOWN)
            .rev()
            .cloned()
            .collect();
        let hidden = run.activity().len().saturating_sub(ACTIVITY_SHOWN);
        let message = run.message().trim().to_string();
        let thinking = (!run.state().is_over() && message.is_empty())
            .then(|| last_paragraph(run.thought()))
            .flatten();
        let muted = cx.theme().muted_foreground;

        card(cx)
            .gap_3()
            .child(header(&self.run, cx))
            .child(progress_bar(run, cx))
            .child(
                h_flex()
                    .gap_4()
                    .pl_8()
                    .text_ui()
                    .text_color(muted)
                    .child(format!("{seen} of {total} files looked at"))
                    .child(format!("{} findings", run.findings())),
            )
            .when(!plan.is_empty(), |this| {
                this.child(section(
                    "Plan",
                    v_flex().gap_1().children(plan.into_iter().map(|step| {
                        let (icon, color) = match step.status {
                            PlanStatus::Completed => {
                                (Icon::new(IconName::CircleCheck).into_any_element(), muted)
                            }
                            PlanStatus::InProgress => (
                                Spinner::new().xsmall().into_any_element(),
                                cx.theme().foreground,
                            ),
                            PlanStatus::Pending => {
                                (Icon::new(IconName::Circle).into_any_element(), muted)
                            }
                        };
                        h_flex()
                            .gap_2()
                            .items_start()
                            .text_ui()
                            .text_color(color)
                            .child(div().pt_0p5().text_color(muted).child(icon))
                            .child(div().flex_1().min_w_0().child(step.content))
                    })),
                    cx,
                ))
            })
            .when(!activity.is_empty(), |this| {
                this.child(section(
                    "Activity",
                    v_flex()
                        .gap_1()
                        .when(hidden > 0, |this| {
                            this.child(
                                div()
                                    .text_meta()
                                    .text_color(muted)
                                    .child(format!("{hidden} earlier steps")),
                            )
                        })
                        .children(activity.into_iter().map(|entry| activity_row(entry, cx))),
                    cx,
                ))
            })
            .when_some(thinking, |this, thought| {
                this.child(section(
                    "Thinking",
                    div()
                        .text_ui()
                        .text_color(muted)
                        .child(SharedString::from(thought)),
                    cx,
                ))
            })
            .when(!message.is_empty(), |this| {
                this.child(section(
                    "Reply",
                    div().text_body().child(TextView::markdown(
                        SharedString::from(format!("agent-reply-{id}")),
                        message,
                    )),
                    cx,
                ))
            })
            .children(permission(&self.run, cx))
            .children(failure(&self.run, cx))
            .children(transcript(&self.run, cx))
    }
}

/// Opens the run's transcript: its prompt and every event the agent sent.
fn transcript(run_entity: &Entity<AgentRun>, cx: &App) -> Option<AnyElement> {
    let path = run_entity.read(cx).log_path()?.to_path_buf();
    Some(
        h_flex()
            .pl_8()
            .child(
                Button::new(SharedString::from(format!(
                    "transcript-{}",
                    run_entity.entity_id().as_u64()
                )))
                .ghost()
                .xsmall()
                .icon(IconName::FileText)
                .label("Transcript")
                .tooltip("The prompt and everything the agent reported")
                .on_click(move |_, _, cx| cx.open_with_system(&path)),
            )
            .into_any_element(),
    )
}

fn card(cx: &App) -> gpui_kit::Div {
    v_flex()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius_lg)
}

fn section(title: &'static str, body: impl IntoElement, cx: &App) -> impl IntoElement {
    v_flex()
        .pl_8()
        .gap_1p5()
        .child(
            div()
                .text_meta()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(title),
        )
        .child(body)
}

/// Agent, model and effort, where the run stands, and Stop or Dismiss.
fn header(run_entity: &Entity<AgentRun>, cx: &App) -> impl IntoElement + use<> {
    let run = run_entity.read(cx);
    let (seen, total) = run.progress();
    let elapsed = format_duration(run.elapsed().as_secs() as u32);
    let state = run.state().clone();
    let (model, effort) = run.model_and_effort();
    let setup = match (model, effort) {
        (Some(model), Some(effort)) => format!(" · {model}, {effort} effort"),
        (Some(model), None) => format!(" · {model}"),
        _ => String::new(),
    };
    let subtitle = match &state {
        RunState::Preparing => format!("checking out the pull request{setup}…"),
        RunState::Starting => format!("starting over ACP{setup}…"),
        RunState::Stopping => "stopping…".to_string(),
        RunState::Failed(_) => format!("stopped after {elapsed}"),
        RunState::Finished(RunOutcome::Completed) => {
            format!("reviewed {seen} of {total} files in {elapsed}{setup}")
        }
        RunState::Finished(RunOutcome::Truncated) => {
            format!("ran out of room after {elapsed}{setup}")
        }
        RunState::Finished(_) => format!("stopped after {elapsed}"),
        RunState::AwaitingPermission => format!("waiting for you · {elapsed}{setup}"),
        RunState::Running => format!("reviewing via ACP · {elapsed}{setup}"),
    };
    let agent = run.agent().login.clone();
    let run = run_entity.clone();
    h_flex()
        .gap_2()
        .text_ui()
        .child(InitialsAvatar::icon(IconName::Bot).size(AvatarSize::Medium))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(agent)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(cx.theme().muted_foreground)
                .child(subtitle),
        )
        .map(|this| {
            if state.is_over() {
                this.child(
                    Button::new(SharedString::from(format!(
                        "dismiss-run-{}",
                        run.entity_id().as_u64()
                    )))
                    .ghost()
                    .small()
                    .label("Dismiss")
                    .on_click(move |_, _, cx| run.update(cx, |run, cx| run.dismiss(cx))),
                )
            } else {
                this.child(
                    Button::new(SharedString::from(format!(
                        "stop-agent-{}",
                        run.entity_id().as_u64()
                    )))
                    .ghost()
                    .small()
                    .icon(IconName::X)
                    .label("Stop")
                    .disabled(state == RunState::Stopping)
                    .on_click(move |_, _, cx| run.update(cx, |run, cx| run.stop(cx))),
                )
            }
        })
}

fn progress_bar(run: &AgentRun, cx: &App) -> impl IntoElement {
    let (seen, total) = run.progress();
    let color = match run.state() {
        RunState::Failed(_) => cx.theme().danger,
        RunState::Finished(_) => cx.theme().success,
        _ => cx.app_theme().accent,
    };
    Progress::new("agent-progress")
        .value(run.fraction() * 100.)
        .color(color)
        .accessibility_label(format!("{seen} of {total} files looked at"))
}

/// What the compact card says the agent is doing: the plan step in progress, else the
/// latest tool call, else the start of its reply.
fn current_step(run: &AgentRun) -> Option<(AnyElement, String)> {
    if run.state().is_over() {
        return None;
    }
    if let Some(step) = run
        .plan()
        .iter()
        .find(|step| step.status == PlanStatus::InProgress)
    {
        let done = run
            .plan()
            .iter()
            .filter(|step| step.status == PlanStatus::Completed)
            .count();
        return Some((
            Spinner::new().xsmall().into_any_element(),
            format!(
                "Step {} of {} · {}",
                done + 1,
                run.plan().len(),
                step.content
            ),
        ));
    }
    if let Some(entry) = run.activity().last() {
        let text = entry
            .command
            .as_ref()
            .map(|command| format!("$ {command}"))
            .unwrap_or_else(|| entry.title.clone());
        return Some((Spinner::new().xsmall().into_any_element(), text));
    }
    last_paragraph(run.thought()).map(|thought| {
        (
            Icon::new(IconName::Sparkles).xsmall().into_any_element(),
            thought,
        )
    })
}

fn activity_row(entry: Activity, cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let status: AnyElement = match entry.status {
        ToolStatus::Completed => Icon::new(IconName::Check)
            .xsmall()
            .text_color(cx.theme().success)
            .into_any_element(),
        ToolStatus::Failed => Icon::new(IconName::X)
            .xsmall()
            .text_color(cx.theme().danger)
            .into_any_element(),
        ToolStatus::Pending | ToolStatus::Running => Spinner::new().xsmall().into_any_element(),
    };
    let kind = match entry.kind {
        Some(ToolKind::Read) => IconName::FileText,
        Some(ToolKind::Search) => IconName::Search,
        Some(ToolKind::Execute) => IconName::Terminal,
        Some(ToolKind::Edit) => IconName::Pencil,
        _ => IconName::Wrench,
    };
    h_flex()
        .gap_2()
        .text_ui()
        .child(div().w_4().flex().justify_center().child(status))
        .child(Icon::new(kind).xsmall().text_color(muted))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(SharedString::from(entry.title.clone())),
        )
        .when_some(
            entry
                .command
                .filter(|command| !entry.title.contains(command.as_str())),
            |this, command| {
                this.child(
                    div()
                        .max_w(gpui_kit::relative(0.45))
                        .truncate()
                        .text_code(cx)
                        .text_color(muted)
                        .child(SharedString::from(command)),
                )
            },
        )
        .child(
            div()
                .text_meta()
                .text_color(muted)
                .child(format_duration(entry.at.as_secs() as u32)),
        )
}

/// The last paragraph of `text`, trimmed, or `None` when there's nothing to show.
fn last_paragraph(text: &str) -> Option<String> {
    text.trim()
        .rsplit("\n\n")
        .next()
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(str::to_string)
}

fn failure(run_entity: &Entity<AgentRun>, cx: &App) -> Option<AnyElement> {
    let run = run_entity.read(cx);
    let RunState::Failed(message) = run.state().clone() else {
        return None;
    };
    Some(
        h_flex()
            .gap_2()
            .pl_8()
            .items_start()
            .text_ui()
            .child(
                div()
                    .pt_0p5()
                    .text_color(cx.theme().danger)
                    .child(Icon::new(IconName::CircleX).small()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(cx.theme().danger)
                    .child(SharedString::from(message)),
            )
            .child(
                Button::new(SharedString::from(format!(
                    "review-again-{}",
                    run_entity.entity_id().as_u64()
                )))
                .outline()
                .small()
                .label("Review again…")
                .tooltip("Pick another model or agent and start over")
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(app_state_ui::actions::ReviewWithAgent), cx)
                }),
            )
            .into_any_element(),
    )
}

/// The agent's request to run something, with Allow once, Always allow and Deny.
fn permission(run_entity: &Entity<AgentRun>, cx: &App) -> Option<AnyElement> {
    let run = run_entity.read(cx);
    let prompt = run.permission()?;
    let title = prompt.title().to_string();
    let command = prompt.command().map(str::to_string);
    let offers_always = prompt.offers(Decision::AllowAlways);
    let agent = run.agent().login.clone();
    let cwd = run.cwd_label().to_string();
    let allow_once = run_entity.clone();
    let allow_always = run_entity.clone();
    let deny = run_entity.clone();
    let heading = match &command {
        Some(_) => format!("{agent} wants to run a command in {cwd}"),
        None => format!("{agent} asks to {}", title.to_lowercase()),
    };
    Some(
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
                            .on_click(move |_, _, cx| {
                                allow_once.update(cx, |run, cx| run.answer(Decision::AllowOnce, cx))
                            }),
                    )
                    .when(offers_always, |this| {
                        this.child(
                            Button::new("allow-always")
                                .ghost()
                                .small()
                                .label("Always allow in this repo")
                                .on_click(move |_, _, cx| {
                                    allow_always
                                        .update(cx, |run, cx| run.answer(Decision::AllowAlways, cx))
                                }),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("deny")
                            .ghost()
                            .small()
                            .icon(IconName::X)
                            .label("Deny")
                            .on_click(move |_, _, cx| {
                                deny.update(cx, |run, cx| run.answer(Decision::Deny, cx))
                            }),
                    ),
            )
            .into_any_element(),
    )
}
