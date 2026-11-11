use std::path::PathBuf;
use std::str::FromStr;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    InitializeRequest, NewSessionRequest, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigSelectOptions, SessionConfigValueId, SessionId,
    SetSessionConfigOptionRequest,
};
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};

/// What a setting controls, as the agent labels it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingKind {
    Model,
    /// Reasoning effort or thinking level.
    Effort,
    Mode,
    Other,
}

/// One value a setting can take.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SettingChoice {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
}

/// A choice the agent lets a client make for a session, such as its model.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct AgentSetting {
    pub id: String,
    pub name: String,
    pub kind: SettingKind,
    /// The value a new session starts with.
    pub current: String,
    pub choices: Vec<SettingChoice>,
}

impl AgentSetting {
    /// The setting a session config option describes; `None` for on/off switches,
    /// which a review has no use for.
    pub(crate) fn from_option(option: &SessionConfigOption) -> Option<Self> {
        let SessionConfigKind::Select(select) = &option.kind else {
            return None;
        };
        let choices = match &select.options {
            SessionConfigSelectOptions::Ungrouped(options) => options.iter().collect::<Vec<_>>(),
            SessionConfigSelectOptions::Grouped(groups) => {
                groups.iter().flat_map(|group| &group.options).collect()
            }
            _ => Vec::new(),
        };
        let kind = match &option.category {
            Some(SessionConfigOptionCategory::Model) => SettingKind::Model,
            Some(SessionConfigOptionCategory::ThoughtLevel) => SettingKind::Effort,
            Some(SessionConfigOptionCategory::Mode) => SettingKind::Mode,
            _ => kind_from_id(&option.id.to_string()),
        };
        Some(Self {
            id: option.id.to_string(),
            name: option.name.clone(),
            kind,
            current: select.current_value.to_string(),
            choices: choices
                .into_iter()
                .map(|choice| SettingChoice {
                    value: choice.value.to_string(),
                    name: choice.name.clone(),
                    description: choice.description.clone(),
                })
                .collect(),
        })
    }

    /// Whether `value` is one of the choices.
    pub fn offers(&self, value: &str) -> bool {
        self.choices.iter().any(|choice| choice.value == value)
    }
}

/// Agents that leave the category out still tend to name the option plainly.
fn kind_from_id(id: &str) -> SettingKind {
    let id = id.to_ascii_lowercase();
    if id.contains("model") {
        SettingKind::Model
    } else if id.contains("effort") || id.contains("thought") || id.contains("reasoning") {
        SettingKind::Effort
    } else if id.contains("mode") {
        SettingKind::Mode
    } else {
        SettingKind::Other
    }
}

/// Starts the agent, opens a session in `cwd`, applies `choices`, and returns the
/// settings the session then offers, before ending the agent. Settings can depend on
/// each other, such as the efforts a model supports. Takes as long as the agent takes
/// to start; `npx` launchers can need several seconds on first use.
pub async fn probe_settings(
    command_line: &str,
    env: &[(String, String)],
    cwd: PathBuf,
    choices: SessionChoices,
) -> Result<Vec<AgentSetting>, String> {
    let agent = agent_for(command_line, env)?;
    agent_client_protocol::Client
        .builder()
        .name("switchfold")
        .connect_with(agent, async move |connection: ConnectionTo<Agent>| {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = connection
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;
            let options = choices
                .apply(
                    &connection,
                    &session.session_id,
                    session.config_options.unwrap_or_default(),
                )
                .await?;
            Ok(options
                .iter()
                .filter_map(AgentSetting::from_option)
                .collect())
        })
        .await
        .map_err(|error| format!("{error}"))
}

/// Sets each `(id, value)` the session offers, in order, and returns the options as
/// the agent reports them after the last change. Values the agent doesn't offer, such
/// as a model it no longer serves, are skipped so the session keeps its default.
pub(crate) async fn apply_settings(
    connection: &ConnectionTo<Agent>,
    session_id: &SessionId,
    mut options: Vec<SessionConfigOption>,
    chosen: &[(String, String)],
) -> Result<Vec<SessionConfigOption>, agent_client_protocol::Error> {
    for (id, value) in chosen {
        let offered = options
            .iter()
            .filter_map(AgentSetting::from_option)
            .any(|setting| &setting.id == id && setting.offers(value));
        if !offered {
            continue;
        }
        let response = connection
            .send_request(SetSessionConfigOptionRequest::new(
                session_id.clone(),
                id.clone(),
                SessionConfigValueId::new(value.clone()),
            ))
            .block_task()
            .await?;
        options = response.config_options;
    }
    Ok(options)
}

/// Mode values that make an agent ask before running commands, best first: Claude
/// Code's `default` (shown as Manual), Codex's `read-only`.
const ASKING_MODES: [&str; 3] = ["default", "read-only", "ask"];

/// What a review asks of a session's settings, by role rather than by the agent's ids.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SessionChoices {
    pub model: Option<String>,
    pub effort: Option<String>,
    /// Switch the agent to a mode that asks before running commands, when it has one.
    pub ask_before_commands: bool,
}

impl SessionChoices {
    /// Picks `model` and `effort`, leaving the agent's mode alone.
    pub fn new(model: Option<String>, effort: Option<String>) -> Self {
        Self {
            model,
            effort,
            ask_before_commands: false,
        }
    }

    /// Also switches to a mode that asks before running commands.
    pub fn asking_before_commands(mut self, ask: bool) -> Self {
        self.ask_before_commands = ask;
        self
    }

    /// Applies the choices to a new session: the asking mode, then the model, then the
    /// effort, which can depend on the model. Choices the agent doesn't offer are left
    /// at the agent's default.
    pub(crate) async fn apply(
        &self,
        connection: &ConnectionTo<Agent>,
        session_id: &SessionId,
        mut options: Vec<SessionConfigOption>,
    ) -> Result<Vec<SessionConfigOption>, agent_client_protocol::Error> {
        let mut wanted: Vec<(SettingKind, String)> = Vec::new();
        if self.ask_before_commands {
            let asking = setting_of(&options, SettingKind::Mode).and_then(|mode| {
                ASKING_MODES
                    .iter()
                    .find(|value| mode.offers(value))
                    .map(|value| value.to_string())
            });
            if let Some(value) = asking {
                wanted.push((SettingKind::Mode, value));
            }
        }
        wanted.extend(self.model.clone().map(|model| (SettingKind::Model, model)));
        wanted.extend(
            self.effort
                .clone()
                .map(|effort| (SettingKind::Effort, effort)),
        );
        for (kind, value) in wanted {
            let Some(setting) = setting_of(&options, kind) else {
                continue;
            };
            if setting.current == value {
                continue;
            }
            options =
                apply_settings(connection, session_id, options, &[(setting.id, value)]).await?;
        }
        Ok(options)
    }
}

/// The first setting of `kind` among `options`.
fn setting_of(options: &[SessionConfigOption], kind: SettingKind) -> Option<AgentSetting> {
    options
        .iter()
        .filter_map(AgentSetting::from_option)
        .find(|setting| setting.kind == kind)
}

/// The agent process for `command_line` with `env` added.
pub(crate) fn agent_for(command_line: &str, env: &[(String, String)]) -> Result<AcpAgent, String> {
    let agent = AcpAgent::from_str(command_line)
        .map_err(|error| format!("The agent command isn’t valid: {error}"))?;
    if env.is_empty() {
        return Ok(agent);
    }
    let mut config = agent.config().clone();
    for (name, value) in env {
        config = config.env(name.clone(), value.clone());
    }
    Ok(AcpAgent::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_without_a_category_are_named_by_id() {
        assert_eq!(kind_from_id("reasoning_effort"), SettingKind::Effort);
        assert_eq!(kind_from_id("model"), SettingKind::Model);
        assert_eq!(kind_from_id("approval-preset"), SettingKind::Other);
    }
}
