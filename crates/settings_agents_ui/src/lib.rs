//! Settings › Agents & editor: ACP agents, what they may do, how files open in Zed, and
//! what Zed can do with Switchfold.

use app_state_ui::StoresExt as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon, IndexPath, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogAction, DialogClose, DialogFooter},
    input::{Input, InputEvent, InputState},
    label::Label,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    notification::Notification,
    select::{Select, SelectEvent, SelectState},
    switch::Switch,
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, ClipboardItem, Context, Entity, FontWeight, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div, rems,
};
use settings_model::{AgentConfig, OpenFilesAt, Preferences, display_path};
use widgets_ui::{SettingsPage, SettingsRow, SettingsSection, StatusTag, Tone};

/// The Agents & editor page.
pub struct AgentsPage {
    zed_command: Entity<InputState>,
    open_files_at: Entity<SelectState<Vec<SharedString>>>,
    zed_mcp_installed: bool,
    zed_task_installed: bool,
    _subscriptions: Vec<Subscription>,
}

impl AgentsPage {
    /// Creates the page.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = cx.stores().settings().clone();
        let preferences = settings.read(cx).preferences().clone();
        let zed_command =
            cx.new(|cx| InputState::new(window, cx).default_value(preferences.zed_command.clone()));
        let options: Vec<SharedString> =
            OpenFilesAt::ALL.iter().map(|o| o.label().into()).collect();
        let selected = OpenFilesAt::ALL
            .iter()
            .position(|o| *o == preferences.open_files_at);
        let open_files_at =
            cx.new(|cx| SelectState::new(options, selected.map(IndexPath::new), window, cx));
        let subscriptions = vec![
            cx.observe(&settings, |_, _, cx| cx.notify()),
            cx.subscribe(&zed_command, |_, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let command = input.read(cx).value().trim().to_string();
                    if !command.is_empty() {
                        cx.stores().settings().clone().update(cx, |settings, cx| {
                            settings.update_preferences(cx, |p| p.zed_command = command)
                        });
                    }
                }
            }),
            cx.subscribe(
                &open_files_at,
                |_, _, event: &SelectEvent<Vec<SharedString>>, cx| {
                    let SelectEvent::Confirm(Some(label)) = event else {
                        return;
                    };
                    if let Some(choice) = OpenFilesAt::ALL
                        .iter()
                        .find(|o| o.label() == label.as_ref())
                    {
                        let choice = *choice;
                        cx.stores().settings().clone().update(cx, |settings, cx| {
                            settings.update_preferences(cx, |p| p.open_files_at = choice)
                        });
                    }
                },
            ),
        ];
        let home = settings.read(cx).home().map(|home| home.to_path_buf());
        Self {
            zed_command,
            open_files_at,
            zed_mcp_installed: home.as_deref().is_some_and(|home| {
                zed_bridge::mcp_status(home) == zed_bridge::McpStatus::Installed
            }),
            zed_task_installed: home
                .as_deref()
                .is_some_and(zed_bridge::comment_task_installed),
            _subscriptions: subscriptions,
        }
    }

    fn render_agent(agent: &AgentConfig, is_default: bool, cx: &App) -> impl IntoElement + use<> {
        let settings = cx.stores().settings().read(cx);
        let home = settings.home();
        let availability = settings.agent_availability(&agent.id);
        let binary = agent
            .binary
            .as_deref()
            .map(|binary| display_path(binary, home))
            .unwrap_or_else(|| agent.command.clone());
        let detail: SharedString = match availability {
            None => format!("{binary} · checking…").into(),
            Some(found) => match (&found.version, found.binary_found, &found.launcher) {
                (Some(version), _, Some(_)) => format!("{binary} · v{version}").into(),
                (None, true, Some(_)) => format!("{binary} · installed").into(),
                (_, _, None) => format!("{} isn’t on PATH · {binary}", agent.command).into(),
                (None, false, Some(_)) => format!("Launched with {}", agent.command_line()).into(),
            },
        };
        let id = agent.id.clone();
        let enabled_id = id.clone();
        let default_id = id.clone();
        let edit_id = id.clone();
        let remove_id = id.clone();
        let command_line = agent.command_line();
        let custom = agent.id.starts_with("custom-");
        let mut row = SettingsRow::new(agent.name.clone()).description(detail);
        if is_default {
            row = row.trailing(StatusTag::new(Tone::Neutral, "Default").icon(IconName::Check));
        }
        row.trailing(
            Switch::new(SharedString::from(format!("agent-enabled-{id}")))
                .checked(agent.is_enabled)
                .accessibility_label(format!("Enable {}", agent.name))
                .on_change(move |checked, _, cx| {
                    let (id, checked) = (enabled_id.clone(), *checked);
                    cx.stores().settings().clone().update(cx, |settings, cx| {
                        settings.set_agent_enabled(&id, checked, cx)
                    });
                }),
        )
        .trailing(
            Button::new(SharedString::from(format!("agent-more-{id}")))
                .ghost()
                .small()
                .icon(IconName::Ellipsis)
                .accessibility_label("More agent actions")
                .dropdown_menu_with_anchor(
                    gpui_kit::Anchor::TopRight,
                    move |menu: PopupMenu, _, _| {
                        let default_id = default_id.clone();
                        let edit_id = edit_id.clone();
                        let remove_id = remove_id.clone();
                        let command_line = command_line.clone();
                        let menu = menu
                            .item(
                                PopupMenuItem::new("Make default")
                                    .disabled(is_default)
                                    .on_click(move |_, _, cx| {
                                        let id = default_id.clone();
                                        cx.stores()
                                            .settings()
                                            .clone()
                                            .update(cx, |settings, cx| {
                                                settings.make_default_agent(&id, cx)
                                            });
                                    }),
                            )
                            .item(PopupMenuItem::new("Edit command…").on_click(
                                move |_, window, cx| {
                                    open_agent_dialog(
                                        Some((edit_id.clone(), command_line.clone())),
                                        window,
                                        cx,
                                    )
                                },
                            ));
                        if custom {
                            menu.separator().item(PopupMenuItem::new("Remove").on_click(
                                move |_, _, cx| {
                                    let id = remove_id.clone();
                                    cx.stores()
                                        .settings()
                                        .clone()
                                        .update(cx, |settings, cx| settings.remove_agent(&id, cx));
                                },
                            ))
                        } else {
                            menu
                        }
                    },
                ),
        )
    }
}

fn preference_row(
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

/// Opens the dialog that adds an agent, or edits `existing` (id, command line).
fn open_agent_dialog(existing: Option<(String, String)>, window: &mut Window, cx: &mut App) {
    let name = cx.new(|cx| InputState::new(window, cx).placeholder("My agent"));
    let command = cx.new(|cx| {
        let state = InputState::new(window, cx).placeholder("npx -y my-acp-agent");
        match &existing {
            Some((_, line)) => state.default_value(line.clone()),
            None => state,
        }
    });
    let editing = existing.map(|(id, _)| id);
    window.open_dialog(cx, move |dialog, _, _| {
        let (name, command, editing) = (name.clone(), command.clone(), editing.clone());
        let (ok_name, ok_command, ok_editing) = (name.clone(), command.clone(), editing.clone());
        let mut body = v_flex().gap_3();
        if editing.is_none() {
            body = body.child(
                v_flex()
                    .gap_1p5()
                    .child(Label::new("Name").font_weight(FontWeight::MEDIUM))
                    .child(Input::new(&name)),
            );
        }
        body =
            body.child(
                v_flex()
                    .gap_1p5()
                    .child(Label::new("Command").font_weight(FontWeight::MEDIUM))
                    .child(Input::new(&command).prefix(Icon::new(IconName::Terminal).small()))
                    .child(div().text_sm().child(
                        "A program that speaks the Agent Client Protocol on stdin and stdout.",
                    )),
            );
        dialog
            .title(if editing.is_some() {
                "Edit agent command"
            } else {
                "Add ACP agent"
            })
            .child(body)
            .on_ok(move |_, _, cx| {
                let line = ok_command.read(cx).value().trim().to_string();
                if line.is_empty() {
                    return false;
                }
                let settings = cx.stores().settings().clone();
                match &ok_editing {
                    Some(id) => settings
                        .update(cx, |settings, cx| settings.set_agent_command(id, &line, cx)),
                    None => {
                        let name = ok_name.read(cx).value().trim().to_string();
                        let name = if name.is_empty() {
                            "Custom agent".to_string()
                        } else {
                            name
                        };
                        settings.update(cx, |settings, cx| settings.add_agent(&name, &line, cx));
                    }
                }
                true
            })
            .footer(
                DialogFooter::new()
                    .gap_2()
                    .child(DialogClose::new().child(Button::new("cancel").ghost().label("Cancel")))
                    .child(
                        DialogAction::new().child(Button::new("save-agent").primary().label(
                            if editing.is_some() {
                                "Save"
                            } else {
                                "Add agent"
                            },
                        )),
                    ),
            )
    });
}

/// Which change to Zed's configuration a button makes.
#[derive(Clone, Copy)]
enum ZedSetup {
    McpServer,
    CommentTask,
}

impl AgentsPage {
    /// Adds Switchfold to Zed's settings or tasks, and reports what happened.
    fn set_up_zed(&mut self, setup: ZedSetup, window: &mut Window, cx: &mut Context<Self>) {
        let home = cx
            .stores()
            .settings()
            .read(cx)
            .home()
            .map(|h| h.to_path_buf());
        let (Some(home), Ok(binary)) = (home, std::env::current_exe()) else {
            window.push_notification(
                Notification::error("Couldn’t find your home folder or Switchfold’s binary."),
                cx,
            );
            return;
        };
        let result = match setup {
            ZedSetup::McpServer => zed_bridge::install_mcp_server(&home, &binary),
            ZedSetup::CommentTask => zed_bridge::install_comment_task(&home, &binary),
        };
        match result {
            Ok(_) => {
                match setup {
                    ZedSetup::McpServer => self.zed_mcp_installed = true,
                    ZedSetup::CommentTask => self.zed_task_installed = true,
                }
                let message = match setup {
                    ZedSetup::McpServer => {
                        "Added Switchfold to Zed’s context servers. Zed’s agent panel can use it now."
                    }
                    ZedSetup::CommentTask => {
                        "Added the task to Zed. Run “task: spawn” in Zed, or bind it to a key."
                    }
                };
                window.push_notification(Notification::success(message), cx);
            }
            Err(error) => {
                window.push_notification(Notification::error(error.to_string()), cx);
            }
        }
        cx.notify();
    }

    fn zed_setup_button(
        &self,
        id: &'static str,
        installed: bool,
        label: &'static str,
        setup: ZedSetup,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        if installed {
            StatusTag::new(Tone::Success, "Added")
                .icon(IconName::Check)
                .into_any_element()
        } else {
            Button::new(id)
                .outline()
                .small()
                .label(label)
                .on_click(
                    cx.listener(move |this, _, window, cx| this.set_up_zed(setup, window, cx)),
                )
                .into_any_element()
        }
    }
}

impl Render for AgentsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = cx.stores().settings().read(cx);
        let preferences = settings.preferences().clone();
        let default_id = settings.default_agent().map(|agent| agent.id.clone());
        let zed_note: SharedString = match settings.zed_version() {
            Some(version) => format!("Found zed {version}").into(),
            None => "Zed wasn’t found at this path".into(),
        };
        let agents: Vec<AgentConfig> = settings.agents().to_vec();
        let rows: Vec<_> = agents
            .iter()
            .map(|agent| {
                Self::render_agent(agent, default_id.as_deref() == Some(agent.id.as_str()), cx)
            })
            .collect();

        SettingsPage::new(
            "agents-page",
            "Agents & editor",
            "Agents connect over the Agent Client Protocol (ACP) and run on this Mac, inside the pull request worktree.",
        )
        .child(
            SettingsSection::new().rows(rows).after(
                Button::new("add-agent")
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("Add ACP agent…")
                    .on_click(|_, window, cx| open_agent_dialog(None, window, cx)),
            ),
        )
        .child(
            SettingsSection::titled("Permissions")
                .row(preference_row(
                    "ask-before-commands",
                    "Ask before an agent runs a command",
                    "Shows Allow once, Always allow and Deny in the thread.",
                    preferences.ask_before_commands,
                    |p, v| p.ask_before_commands = v,
                ))
                .row(preference_row(
                    "agents-post-directly",
                    "Add agent findings to your review automatically",
                    "When off, findings stay drafts until you add them to a review.",
                    preferences.agents_post_directly,
                    |p, v| p.agents_post_directly = v,
                ))
                .row(preference_row(
                    "share-discussion",
                    "Share the pull request discussion with agents",
                    "Includes comments and review threads in the agent context.",
                    preferences.share_discussion_with_agents,
                    |p, v| p.share_discussion_with_agents = v,
                )),
        )
        .child(
            SettingsSection::titled("Editor")
                .row(
                    SettingsRow::new("Zed command").description(zed_note).trailing(
                        div().w(rems(15.)).child(
                            Input::new(&self.zed_command)
                                .small()
                                .prefix(Icon::new(IconName::Terminal).small()),
                        ),
                    ),
                )
                .row(
                    SettingsRow::new("Open files at")
                        .description("Which revision Open in Zed uses for files in a pull request.")
                        .trailing(div().w(rems(11.25)).child(Select::new(&self.open_files_at).small())),
                ),
        )
        .child(
            SettingsSection::titled("Zed")
                .row(
                    SettingsRow::new("Switchfold in Zed’s agent panel")
                        .description(
                            "Zed’s agent can read the open pull request’s threads and findings and draft pending comments. It never submits a review. Adds an entry to context_servers in ~/.config/zed/settings.json.",
                        )
                        .trailing(self.zed_setup_button(
                            "zed-add-mcp",
                            self.zed_mcp_installed,
                            "Add to Zed",
                            ZedSetup::McpServer,
                            cx,
                        )),
                )
                .row(
                    SettingsRow::new("Comment on a line from Zed")
                        .description(
                            "A Zed task that opens a comment on the cursor’s line here. Adds it to ~/.config/zed/tasks.json.",
                        )
                        .trailing(self.zed_setup_button(
                            "zed-add-task",
                            self.zed_task_installed,
                            "Add task",
                            ZedSetup::CommentTask,
                            cx,
                        )),
                )
                .row(
                    SettingsRow::new("Key for the task")
                        .description("Paste into Zed’s keymap (zed: open keymap) to comment with ⌘⌥⇧C.")
                        .trailing(
                            Button::new("zed-copy-keybinding")
                                .ghost()
                                .small()
                                .icon(IconName::Copy)
                                .label("Copy keymap entry")
                                .on_click(|_, window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        zed_bridge::comment_task_keybinding(),
                                    ));
                                    window.push_notification(
                                        Notification::success("Copied the keymap entry."),
                                        cx,
                                    );
                                }),
                        ),
                ),
        )
    }
}
