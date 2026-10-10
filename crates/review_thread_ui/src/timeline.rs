use std::rc::Rc;

use app_state_ui::StoresExt as _;
use app_state_ui::actions::{ReviewWithAgent, SubmitComment};
use diff_view_ui::{DiffMode, DiffRow, FileDiffRow};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Textarea, TextareaState},
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    notification::Notification,
    scroll::ScrollableElement as _,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Styled as _, Window, div, list,
    prelude::FluentBuilder as _,
};
use pr_model::Timestamp;
use review_model::{
    AgentFinding, FindingState, PullRequestDetail, ReviewThread, ReviewVerdict, TimelineItem,
};
use theme_ui::{ActiveAppTheme as _, TypographyExt as _};
use widgets_ui::{AvatarSize, EditMenu, FilterTabs, InitialsAvatar, StatusTag, Tone};

use crate::cards::{CheckRow, CommentCard, CommitRow, EventRow, actor_avatar};
use crate::rows::TimelineRow;
use crate::{ReviewThreadView, TimelineFilter, filters};

impl ReviewThreadView {
    pub(crate) fn render_toolbar(
        &self,
        detail: &PullRequestDetail,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let unresolved = filters::unresolved_count(detail);
        let running = agent_review_ui::running_count(&self.key, cx);
        let tabs =
            TimelineFilter::ALL
                .iter()
                .fold(FilterTabs::new("timeline-filter"), |tabs, filter| {
                    let count = match filter {
                        TimelineFilter::Unresolved if unresolved > 0 => Some(unresolved),
                        TimelineFilter::Reviews if running > 0 => Some(running),
                        _ => None,
                    };
                    tabs.tab(filter.label(), count)
                });
        let selected = TimelineFilter::ALL
            .iter()
            .position(|filter| *filter == self.filter)
            .unwrap_or_default();
        let commits: Vec<(String, String, String)> = detail
            .commits
            .iter()
            .map(|commit| {
                (
                    commit.sha.clone(),
                    commit.short_sha().to_string(),
                    commit.message.clone(),
                )
            })
            .collect();
        let all_label = format!("All commits ({})", commits.len());
        let picked = self.commit.as_ref().map(|commit| commit.sha.clone());
        let commit_label = match &self.commit {
            Some(commit) => {
                let mut message: String = commit.message.chars().take(24).collect();
                if commit.message.chars().count() > 24 {
                    message.push('…');
                }
                format!("{} {message}", commit.short_sha)
            }
            None => all_label.clone(),
        };
        let menu_view = cx.entity();
        h_flex()
            .flex_shrink_0()
            .min_w_0()
            .overflow_hidden()
            .py_2()
            .px_6()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                tabs.selected(selected)
                    .on_select(cx.listener(|this, ix: &usize, _, cx| {
                        this.show_all_commits(cx);
                        this.set_filter(TimelineFilter::ALL[*ix]);
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .child(
                Button::new("commit-range")
                    .outline()
                    .small()
                    .max_w(gpui_kit::rems(22.))
                    .dropdown_caret(true)
                    .label(commit_label)
                    .dropdown_menu_with_anchor(
                        gpui_kit::Anchor::TopRight,
                        move |menu: PopupMenu, _, _| {
                            let all_view = menu_view.clone();
                            let menu = menu
                                .item(
                                    PopupMenuItem::new(all_label.clone())
                                        .checked(picked.is_none())
                                        .on_click(move |_, _, cx| {
                                            all_view
                                                .update(cx, |this, cx| this.show_all_commits(cx))
                                        }),
                                )
                                .separator();
                            commits.iter().fold(menu, |menu, (sha, short, message)| {
                                let view = menu_view.clone();
                                let sha = sha.clone();
                                menu.item(
                                    PopupMenuItem::new(format!("{short}  {message}"))
                                        .checked(picked.as_deref() == Some(sha.as_str()))
                                        .on_click(move |_, _, cx| {
                                            view.update(cx, |this, cx| {
                                                this.show_commit(sha.clone(), cx)
                                            })
                                        }),
                                )
                            })
                        },
                    ),
            )
            .child(
                FilterTabs::new("diff-mode")
                    .tab("Unified", None)
                    .tab("Split", None)
                    .selected(if self.mode == DiffMode::Split { 1 } else { 0 })
                    .on_select(cx.listener(|this, ix: &usize, _, cx| {
                        this.mode = if *ix == 1 {
                            DiffMode::Split
                        } else {
                            DiffMode::Unified
                        };
                        cx.notify();
                    })),
            )
    }

    pub(crate) fn render_timeline(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let rem = window.rem_size();
        if self.measured_rem.replace(rem).is_some_and(|old| old != rem) {
            self.list_state.remeasure();
        }
        self.sync_rows(cx);
        div()
            .id("timeline")
            .flex_1()
            .min_h_0()
            .child(
                list(
                    self.list_state.clone(),
                    cx.processor(|this, ix: usize, window, cx| this.render_row(ix, window, cx)),
                )
                .size_full(),
            )
            .vertical_scrollbar(&self.list_state)
    }

    fn render_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (Some(detail), Some(row)) = (self.detail.clone(), self.rows.get(ix).copied()) else {
            return div().into_any_element();
        };
        let now = cx.stores().pull_requests().read(cx).now();
        let element = match row {
            TimelineRow::Item(item) => {
                let item = self.items[item].clone();
                self.render_item(&detail, &item, now, cx)
            }
            TimelineRow::File { item, row, last } => {
                let TimelineItem::File { path } = self.items[item].clone() else {
                    return div().into_any_element();
                };
                self.render_file_row(&detail, &path, row, last, ix, now, window, cx)
            }
            TimelineRow::Empty => self.render_empty().into_any_element(),
            TimelineRow::RunDetails(ix) => match self.runs.get(ix) {
                Some(run) => agent_review_ui::AgentRunDetails::new(run.clone()).into_any_element(),
                None => div().into_any_element(),
            },
            TimelineRow::AgentRun(ix) => match self.active_runs.get(ix) {
                Some(run) => agent_review_ui::AgentRunCard::new(run.clone()).into_any_element(),
                None => div().into_any_element(),
            },
            TimelineRow::ReviewsHeader => self.render_reviews_header(cx).into_any_element(),
            TimelineRow::StartForm => self.start_form(window, cx).into_any_element(),
            TimelineRow::CommitBanner => self.render_commit_banner(cx).into_any_element(),
            TimelineRow::Composer => self.render_composer(cx).into_any_element(),
        };
        let last = ix + 1 == self.rows.len();
        div()
            .px_6()
            .when(row.starts_entry(), |this| {
                if ix == 0 { this.pt_5() } else { this.pt_3() }
            })
            .when(last, |this| this.pb_8())
            .child(element)
            .into_any_element()
    }

    fn render_item(
        &mut self,
        detail: &PullRequestDetail,
        item: &TimelineItem,
        now: Timestamp,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match item {
            TimelineItem::Description => {
                let author = review_model::Actor::person(detail.summary.author.clone());
                CommentCard::new(
                    "description",
                    author,
                    format!("opened {}", detail.opened_at.long_age(now)),
                    detail.body.clone(),
                )
                .repo(self.key.repo().clone())
                .on_quote(self.quote_in_composer_handler(cx))
                .action(
                    Button::new("description-reply")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Reply)
                        .label("Reply")
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.reply_to_description(window, cx)
                            }),
                        ),
                )
                .into_any_element()
            }
            TimelineItem::ReviewRequested {
                actor,
                reviewers,
                at,
            } => {
                let names: Vec<&str> = reviewers.iter().map(|r| r.display_name()).collect();
                EventRow::new(
                    InitialsAvatar::icon(IconName::Users),
                    actor,
                    format!("requested review from {}", join_names(&names)),
                )
                .age(at.short_age(now))
                .into_any_element()
            }
            TimelineItem::Pushed { actor, commits, at } => {
                let noun = if commits.len() == 1 {
                    "commit"
                } else {
                    "commits"
                };
                v_flex()
                    .child(
                        EventRow::new(
                            InitialsAvatar::icon(IconName::GitCommitHorizontal),
                            actor,
                            format!("pushed {} {noun}", commits.len()),
                        )
                        .age(at.short_age(now)),
                    )
                    .children(commits.iter().filter_map(|sha| {
                        detail
                            .commits
                            .iter()
                            .find(|commit| &commit.sha == sha)
                            .map(|commit| CommitRow::new(commit.clone()))
                    }))
                    .into_any_element()
            }
            TimelineItem::File { .. } => div().into_any_element(),
            TimelineItem::AgentReviewed(summary) => EventRow::new(
                InitialsAvatar::icon(IconName::Bot),
                &summary.agent,
                format!(
                    "reviewed {} files via ACP · {} findings, {} added to your review",
                    summary.files, summary.findings, summary.added
                ),
            )
            .age(summary.finished_at.short_age(now))
            .into_any_element(),
            TimelineItem::Finding { id } => match detail.finding(id) {
                Some(finding) => self.render_finding(finding, cx),
                None => div().into_any_element(),
            },
            TimelineItem::Reviewed { actor, verdict, at } => {
                let (icon, color) = match verdict {
                    ReviewVerdict::Approve => (IconName::CircleCheck, cx.theme().success),
                    ReviewVerdict::RequestChanges => (IconName::CircleX, cx.theme().danger),
                    ReviewVerdict::Comment => {
                        (IconName::MessageSquare, cx.theme().muted_foreground)
                    }
                };
                EventRow::new(
                    Icon::new(icon).text_color(color),
                    actor,
                    verdict.timeline_text(),
                )
                .age(at.short_age(now))
                .into_any_element()
            }
            TimelineItem::Check { id } => match detail.check(id) {
                Some(check) => {
                    let key = self.key.clone();
                    let check_id = check.id.clone();
                    CheckRow::new(check.clone())
                        .details(
                            Button::new(SharedString::from(format!("check-details-{}", check.id)))
                                .ghost()
                                .small()
                                .icon(IconName::Terminal)
                                .label("Details")
                                .on_click(move |_, window, cx| {
                                    check_log_ui::open(key.clone(), check_id.clone(), window, cx)
                                }),
                        )
                        .into_any_element()
                }
                None => div().into_any_element(),
            },
            TimelineItem::Commented { comment } => CommentCard::new(
                SharedString::from(comment.id.clone()),
                comment.author.clone(),
                format!("commented {}", comment.created_at.long_age(now)),
                comment.body.clone(),
            )
            .repo(self.key.repo().clone())
            .on_quote(self.quote_in_composer_handler(cx))
            .into_any_element(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_file_row(
        &mut self,
        detail: &PullRequestDetail,
        path: &str,
        row: DiffRow,
        last: bool,
        row_ix: usize,
        now: Timestamp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(file) = self.files.get(path).cloned() else {
            return div().into_any_element();
        };
        let highlights = match row {
            DiffRow::Line { .. } | DiffRow::Split { .. } => self.highlights_for(path, cx),
            _ => Rc::default(),
        };
        let mut element = FileDiffRow::new(file, highlights, row)
            .last(last)
            .collapsed(self.collapsed.contains(path))
            .viewed(detail.is_viewed(path))
            .document_order(row_ix as u64);
        match row {
            DiffRow::Header => {
                let menu_view = cx.entity();
                let menu_path = path.to_string();
                element = element.menu(move |menu, _, cx| {
                    ReviewThreadView::file_menu(&menu_view, &menu_path, menu, cx)
                });
                let toggle_path = path.to_string();
                let viewed_path = path.to_string();
                let zed_path = path.to_string();
                element = element
                    .on_toggle_collapsed(cx.listener(move |this, _, _, cx| {
                        if !this.collapsed.remove(&toggle_path) {
                            this.collapsed.insert(toggle_path.clone());
                        }
                        cx.notify();
                    }))
                    .on_viewed(cx.listener(move |this, viewed: &bool, _, cx| {
                        this.set_viewed(&viewed_path, *viewed, cx)
                    }))
                    .on_open_in_zed(cx.listener(move |this, _, window, cx| {
                        this.open_in_zed(Some(zed_path.clone()), None, window, cx)
                    }));
            }
            DiffRow::Line { .. } | DiffRow::Split { .. } => {
                let comment_path = path.to_string();
                let view = cx.entity();
                let (menu_view, menu_path) = (view.clone(), path.to_string());
                element = element
                    .on_comment_line(move |line, window, cx| {
                        let path = comment_path.clone();
                        view.update(cx, |this, cx| {
                            this.start_line_comment(path, line, window, cx)
                        })
                    })
                    .line_menu(move |menu, target, _, cx| {
                        ReviewThreadView::diff_line_menu(&menu_view, &menu_path, menu, target, cx)
                    });
            }
            DiffRow::Threads(line) => {
                let mut threads: Vec<AnyElement> = detail
                    .threads
                    .iter()
                    .filter(|thread| thread.path == path && thread.line == line)
                    .map(|thread| self.render_thread(thread, now, window, cx))
                    .collect();
                if let Some((new_path, new_line, input)) = &self.new_thread
                    && new_path == path
                    && *new_line == line
                {
                    let input = input.clone();
                    threads.push(self.render_editor(
                        "line-comment",
                        &input,
                        "Comment",
                        |this, _, cx| this.submit_line_comment(cx),
                        |this, _, _| this.new_thread = None,
                        cx,
                    ));
                }
                element = element.threads(threads);
            }
            DiffRow::Hunk(_) | DiffRow::Binary => {}
        }
        element.into_any_element()
    }

    fn render_thread(
        &mut self,
        thread: &ReviewThread,
        now: Timestamp,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let thread_id = thread.id.clone();
        if thread.is_resolved {
            let starter = thread
                .starter()
                .map(|a| a.login.clone())
                .unwrap_or_default();
            return h_flex()
                .gap_2()
                .px_3()
                .h_8()
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius_lg)
                .text_ui()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(IconName::Check).small())
                .child(
                    div()
                        .flex_1()
                        .child(format!("Resolved thread started by {starter}")),
                )
                .child(
                    Button::new(SharedString::from(format!("reopen-{thread_id}")))
                        .ghost()
                        .xsmall()
                        .label("Reopen")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let key = this.key.clone();
                            let id = thread_id.clone();
                            this.store(cx)
                                .update(cx, |store, cx| store.set_resolved(&key, &id, false, cx));
                        })),
                )
                .into_any_element();
        }
        let reply_input = self.reply_inputs.get(&thread.id).cloned();
        let label = crate::triage::thread(&self.key, &thread.id, cx);
        let mut cards: Vec<AnyElement> = Vec::new();
        let last = thread.comments.len().saturating_sub(1);
        for (ix, comment) in thread.comments.iter().enumerate() {
            let mut card = CommentCard::new(
                SharedString::from(comment.id.clone()),
                comment.author.clone(),
                format!("commented {}", comment.created_at.long_age(now)),
                comment.body.clone(),
            )
            .pending(comment.is_pending)
            .repo(self.key.repo().clone())
            .on_quote({
                let (view, thread_id) = (cx.entity(), thread.id.clone());
                move |text, window, cx| {
                    view.update(cx, |this, cx| {
                        this.quote_in_reply(&thread_id, text, window, cx)
                    })
                }
            });
            if ix == 0
                && let Some(label) = label
            {
                card = card.badge(crate::triage::thread_tag(&label));
                if label.looks_addressed() {
                    card = card.badge(StatusTag::new(Tone::Success, "Looks addressed"));
                }
            }
            if ix == last {
                let resolve_id = thread.id.clone();
                let reply_focus = reply_input.clone();
                let (ask_path, ask_line) = (thread.path.clone(), thread.line);
                let ask_subject = thread_subject(thread);
                card = card
                    .action(
                        Button::new(SharedString::from(format!("reply-{}", comment.id)))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Reply)
                            .label("Reply")
                            .on_click(move |_, window, cx| {
                                if let Some(input) = &reply_focus {
                                    input.update(cx, |input, cx| input.focus(window, cx));
                                }
                            }),
                    )
                    .action(
                        Button::new(SharedString::from(format!("resolve-{}", comment.id)))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Check)
                            .label("Resolve")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let key = this.key.clone();
                                let id = resolve_id.clone();
                                this.store(cx).update(cx, |store, cx| {
                                    store.set_resolved(&key, &id, true, cx)
                                });
                            })),
                    )
                    .action(
                        Button::new(SharedString::from(format!("ask-zed-{}", comment.id)))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Sparkles)
                            .label("Ask Zed’s agent")
                            .tooltip("Open this line in Zed with the thread as an agent prompt")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.ask_zed_agent(&ask_path, ask_line, &ask_subject, window, cx)
                            })),
                    );
            }
            cards.push(card.into_any_element());
        }
        v_flex()
            .gap_2()
            .children(cards)
            .when_some(reply_input, |this, input| {
                let submit_id = thread_id.clone();
                this.child(self.render_editor(
                    format!("reply-{thread_id}"),
                    &input,
                    "Reply",
                    move |this, window, cx| this.submit_reply(&submit_id, window, cx),
                    |_, _, _| {},
                    cx,
                ))
            })
            .into_any_element()
    }

    /// A bordered multi-line editor with Cancel and a submit button, like the comment
    /// composer. Cancel clears the text and leaves the field, then runs `on_cancel`.
    fn render_editor(
        &self,
        id: impl Into<SharedString>,
        input: &Entity<TextareaState>,
        submit_label: &'static str,
        on_submit: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        on_cancel: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id: SharedString = id.into();
        let has_text = !input.read(cx).value().trim().is_empty();
        let clear = input.clone();
        v_flex()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .bg(cx.theme().background)
            .child(div().px_1().pt_1().child(EditMenu::new(
                SharedString::from(format!("{id}-edit-menu")),
                input,
                Textarea::new(input).appearance(false),
            )))
            .child(
                h_flex()
                    .px_3()
                    .pb_2()
                    .gap_2()
                    .text_meta()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("Markdown · ⌘↵ to {}", submit_label.to_lowercase()))
                    .child(div().flex_1())
                    .child(
                        Button::new(SharedString::from(format!("{id}-cancel")))
                            .ghost()
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                clear.update(cx, |input, cx| input.set_value("", window, cx));
                                window.focus(&this.focus_handle, cx);
                                on_cancel(this, window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("{id}-submit")))
                            .primary()
                            .small()
                            .label(submit_label)
                            .disabled(!has_text)
                            .on_click(
                                cx.listener(move |this, _, window, cx| on_submit(this, window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_finding(&mut self, finding: &AgentFinding, cx: &mut Context<Self>) -> AnyElement {
        let id = finding.id.clone();
        if finding.state == FindingState::Dismissed {
            return h_flex()
                .gap_2()
                .px_3()
                .h_8()
                .text_ui()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(IconName::Bot).small())
                .child(
                    div()
                        .flex_1()
                        .child(format!("Dismissed finding · {}", finding.anchor_label())),
                )
                .child(
                    Button::new(SharedString::from(format!("restore-{id}")))
                        .ghost()
                        .xsmall()
                        .label("Restore")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_finding_state(&id, FindingState::Draft, cx)
                        })),
                )
                .into_any_element();
        }
        let editing = self
            .editing_finding
            .as_ref()
            .filter(|(editing_id, _)| editing_id == &finding.id)
            .map(|(_, input)| input.clone());
        let (tag, actions): (StatusTag, Vec<AnyElement>) = match finding.state {
            FindingState::Draft => {
                let add_id = id.clone();
                let edit_id = id.clone();
                let edit_body = finding.body.clone();
                let dismiss_id = id.clone();
                (
                    StatusTag::new(Tone::Neutral, "Not posted").icon(IconName::Pencil),
                    vec![
                        Button::new(SharedString::from(format!("add-{id}")))
                            .small()
                            .icon(IconName::MessageSquarePlus)
                            .label("Add to review")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_finding_state(&add_id, FindingState::Added, cx)
                            }))
                            .into_any_element(),
                        Button::new(SharedString::from(format!("edit-{id}")))
                            .ghost()
                            .small()
                            .icon(IconName::Pencil)
                            .label("Edit")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.edit_finding(&edit_id, &edit_body, window, cx)
                            }))
                            .into_any_element(),
                        Button::new(SharedString::from(format!("dismiss-{id}")))
                            .ghost()
                            .small()
                            .icon(IconName::X)
                            .label("Dismiss")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_finding_state(&dismiss_id, FindingState::Dismissed, cx)
                            }))
                            .into_any_element(),
                    ],
                )
            }
            _ => {
                let remove_id = id.clone();
                (
                    StatusTag::new(Tone::Accent, "In your review")
                        .icon(IconName::MessageSquarePlus),
                    vec![
                        Button::new(SharedString::from(format!("remove-{id}")))
                            .ghost()
                            .small()
                            .icon(IconName::X)
                            .label("Remove from review")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_finding_state(&remove_id, FindingState::Draft, cx)
                            }))
                            .into_any_element(),
                    ],
                )
            }
        };
        let rated = crate::triage::finding(&self.key, &finding.id, cx);
        let noise = rated.is_some_and(|rated| rated.is_likely_noise());
        let (ask_path, ask_line) = (finding.path.clone(), finding.line);
        let ask_subject = format!(
            "{} flagged this in its review:\n\n{}",
            finding.agent.display_name(),
            finding.body
        );
        let mut actions = actions;
        actions.push(
            Button::new(SharedString::from(format!("ask-zed-{id}")))
                .ghost()
                .small()
                .icon(IconName::Sparkles)
                .label("Ask Zed’s agent")
                .tooltip("Open this line in Zed with the finding as an agent prompt")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.ask_zed_agent(&ask_path, ask_line, &ask_subject, window, cx)
                }))
                .into_any_element(),
        );
        v_flex()
            .gap_2()
            .p_3()
            // Likely noise stays readable but steps back.
            .when(noise, |this| this.opacity(0.55))
            .border_1()
            .border_dashed()
            .border_color(cx.theme().muted_foreground.opacity(0.6))
            .rounded(cx.theme().radius_lg)
            .child(
                h_flex()
                    .gap_2()
                    .text_ui()
                    .child(actor_avatar(&finding.agent, AvatarSize::Medium))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(finding.agent.login.clone())),
                    )
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("finding · {}", finding.anchor_label())),
                    )
                    .child(tag)
                    .children(rated.map(|rated| crate::triage::finding_tag(&rated)))
                    .when(noise, |this| {
                        this.child(StatusTag::new(Tone::Neutral, "Likely noise"))
                    }),
            )
            .child(match editing {
                Some(input) => div()
                    .pl_8()
                    .child(self.render_editor(
                        format!("edit-finding-{id}"),
                        &input,
                        "Save",
                        |this, _, cx| this.save_finding_edit(cx),
                        |this, _, _| this.editing_finding = None,
                        cx,
                    ))
                    .into_any_element(),
                None => div()
                    .pl_8()
                    .text_body()
                    .child(markdown_ui::github_markdown(
                        SharedString::from(format!("finding-body-{id}")),
                        &finding.body,
                        Some(self.key.repo()),
                    ))
                    .into_any_element(),
            })
            .child(h_flex().pl_6().gap_1().children(actions))
            .into_any_element()
    }

    /// The Reviews tab's heading: how many agents are at work, and Review with agent.
    fn render_reviews_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let running = agent_review_ui::running_count(&self.key, cx);
        let has_agent = cx.stores().settings().read(cx).default_agent().is_some();
        let open = self.start_form_open(cx);
        let summary = match running {
            0 => "Agent reviews of this session, newest first.".to_string(),
            1 => "1 agent reviewing. Start another to compare.".to_string(),
            n => format!("{n} agents reviewing in parallel."),
        };
        h_flex()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ui()
                    .text_color(cx.theme().muted_foreground)
                    .child(summary),
            )
            .child(if open {
                Button::new("reviews-hide-start")
                    .ghost()
                    .small()
                    .label("Hide")
                    .on_click(cx.listener(|this, _, _, cx| this.hide_agent_review(cx)))
            } else {
                Button::new("reviews-start-agent")
                    .outline()
                    .small()
                    .icon(IconName::Sparkles)
                    .label(if running > 0 {
                        "Add an agent…"
                    } else {
                        "Review with agent…"
                    })
                    .disabled(!has_agent)
                    .tooltip_with_action("Review with agent", &ReviewWithAgent, None)
                    .on_click(cx.listener(|this, _, window, cx| this.open_agent_review(window, cx)))
            })
    }

    /// What an empty filter says.
    fn render_empty(&self) -> impl IntoElement + use<> {
        let (icon, title, body) = match self.filter {
            TimelineFilter::Reviews => (
                IconName::Bot,
                "No reviews yet",
                "Reviews people post and agent reviews you start appear here, with their progress.",
            ),
            TimelineFilter::Checks => (
                IconName::CircleCheck,
                "No checks",
                "No check runs reported for the head commit.",
            ),
            TimelineFilter::Changes => (
                IconName::FileCode,
                "No changes",
                "This pull request changes no files.",
            ),
            _ => (
                IconName::CircleCheck,
                "Nothing unresolved",
                "Every thread is resolved and every agent finding has a decision.",
            ),
        };
        widgets_ui::EmptyState::new(icon, title, body)
    }

    /// Brings the composer, the last row, on screen and focuses it.
    fn reply_to_description(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_filter(TimelineFilter::Everything);
        self.sync_rows(cx);
        self.list_state.scroll_to_end();
        self.composer
            .update(cx, |composer, cx| composer.focus(window, cx));
        cx.notify();
    }

    fn render_composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_text = !self.composer.read(cx).value().trim().is_empty();
        v_flex()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .child(div().px_1().pt_1().child(EditMenu::new(
                "composer-edit-menu",
                &self.composer,
                Textarea::new(&self.composer).appearance(false).h_16(),
            )))
            .child(
                h_flex()
                    .px_3()
                    .pb_2()
                    .gap_2()
                    .text_meta()
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(IconName::Sparkles).small())
                    .child("Markdown · ⌘↵ to comment")
                    .child(div().flex_1())
                    .child(
                        Button::new("composer-cancel")
                            .ghost()
                            .small()
                            .label("Cancel")
                            .disabled(!has_text)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.composer
                                    .update(cx, |composer, cx| composer.set_value("", window, cx))
                            })),
                    )
                    .child(
                        Button::new("composer-submit")
                            .small()
                            .label("Comment")
                            .disabled(!has_text)
                            .tooltip_with_action("Post comment", &SubmitComment, None)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.post_comment(window, cx)),
                            ),
                    ),
            )
    }

    pub(crate) fn check_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = cx.stores().settings().read(cx);
        let clone = settings
            .clone_path(self.key.repo())
            .map(|p| p.to_path_buf());
        let worktree = settings.preferences().worktree_per_pull_request;
        let home = settings.home().map(|p| p.to_path_buf());
        let Some(detail) = self.detail.clone() else {
            return;
        };
        let Some(clone) = clone.filter(|clone| clone.is_dir()) else {
            window.push_notification(
                Notification::warning(format!(
                    "{} has no local clone to check out into. Map one in Settings › Repositories.",
                    self.key.repo()
                )),
                cx,
            );
            return;
        };
        let request =
            git_worktree::Checkout::new(clone, detail.summary.head_ref.clone(), self.key.number());
        let request = if worktree {
            request.in_worktree(home.unwrap_or_default().join(".switchfold/worktrees"))
        } else {
            request
        };
        let task = cx.background_spawn(async move { git_worktree::check_out(&request) });
        cx.spawn_in(window, async move |_, cx| {
            let result = task.await;
            cx.update(|window, cx| match result {
                Ok(path) => window.push_notification(
                    Notification::success(format!("Checked out into {}.", path.display())),
                    cx,
                ),
                Err(error) => window.push_notification(Notification::error(error.to_string()), cx),
            })
            .ok();
        })
        .detach();
    }
}

pub(crate) fn join_names(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The color the timeline uses for the viewer's pending work.
pub(crate) fn pending_color(cx: &App) -> gpui_kit::Hsla {
    cx.app_theme().accent
}

/// A thread as a prompt for an agent: who said what, oldest first.
fn thread_subject(thread: &ReviewThread) -> String {
    let comments: Vec<String> = thread
        .comments
        .iter()
        .map(|comment| format!("{} wrote:\n{}", comment.author.display_name(), comment.body))
        .collect();
    format!("A review thread there says:\n\n{}", comments.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::join_names;

    #[test]
    fn names_join_with_a_final_and() {
        assert_eq!(
            join_names(&["tanya", "mark-v", "Claude Code"]),
            "tanya, mark-v and Claude Code"
        );
        assert_eq!(join_names(&["tanya"]), "tanya");
    }
}
