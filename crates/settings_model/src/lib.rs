//! User-level configuration: GitHub accounts, repository-to-clone mappings, ACP agents
//! and preferences. Serializable so the app can persist it; no UI or I/O here.

use std::path::{Path, PathBuf};

use pr_model::{RepoSlug, Timestamp};
use serde::{Deserialize, Serialize};

/// How an account's credentials were obtained.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountSource {
    /// Borrowed from the GitHub CLI's keyring entry; `gh` owns the token.
    GhCli,
    /// A personal access token stored in the macOS Keychain.
    Token { expires_in_days: Option<u32> },
}

/// A GitHub account used to load pull requests.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Account {
    pub login: String,
    /// The profile name, used for avatar initials when present.
    pub name: Option<String>,
    pub host: String,
    pub source: AccountSource,
}

impl Account {
    /// Builds an account.
    pub fn new(login: impl Into<String>, host: impl Into<String>, source: AccountSource) -> Self {
        Self {
            login: login.into(),
            name: None,
            host: host.into(),
            source,
        }
    }

    /// Sets the profile name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The name avatars take their initials from: the profile name, else the login.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.login)
    }

    /// Whether the token expires within two weeks.
    pub fn is_expiring_soon(&self) -> bool {
        matches!(
            self.source,
            AccountSource::Token {
                expires_in_days: Some(days)
            } if days <= 14
        )
    }

    /// `github.com · from GitHub CLI (gh)` or `ghe.example-corp.com · token · expires in 9 days`.
    pub fn description(&self) -> String {
        match &self.source {
            AccountSource::GhCli => format!("{} · from GitHub CLI (gh)", self.host),
            AccountSource::Token {
                expires_in_days: Some(days),
            } => format!("{} · token · expires in {days} days", self.host),
            AccountSource::Token {
                expires_in_days: None,
            } => format!("{} · token", self.host),
        }
    }
}

/// Where a repository's local clone stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloneStatus {
    Fetched {
        at: Timestamp,
        worktrees: u32,
    },
    Fetching,
    /// No local clone; diffs come from the GitHub API.
    NotCloned,
    /// The mapped folder does not exist any more.
    FolderNotFound,
}

/// A GitHub repository and the local clone diffs, files and agents read from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RepositoryMapping {
    pub slug: String,
    pub local_path: Option<PathBuf>,
}

impl RepositoryMapping {
    /// Builds a mapping; `local_path` is `None` when the repository is not cloned.
    pub fn new(slug: &RepoSlug, local_path: Option<PathBuf>) -> Self {
        Self {
            slug: slug.to_string(),
            local_path,
        }
    }

    /// The repository this mapping is for, if the stored slug is well formed.
    pub fn repo(&self) -> Option<RepoSlug> {
        RepoSlug::parse(&self.slug)
    }
}

/// `~/src/switchfold` for paths inside `home`, the path itself otherwise.
pub fn display_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(relative) if relative.as_os_str().is_empty() => "~".into(),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

/// An ACP agent Switchfold can launch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    /// The executable that speaks ACP over stdio, or a launcher such as `npx`.
    pub command: String,
    pub args: Vec<String>,
    /// The agent's own binary, shown in Settings and used to report its version.
    pub binary: Option<PathBuf>,
    pub is_enabled: bool,
    /// The model last picked for reviews, as the agent names its value; `None` keeps
    /// the agent's default.
    #[serde(default)]
    pub model: Option<String>,
    /// The reasoning effort last picked for reviews, as the agent names its value.
    #[serde(default)]
    pub effort: Option<String>,
}

impl AgentConfig {
    /// Builds an enabled agent.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        command: impl Into<String>,
        args: Vec<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            command: command.into(),
            args,
            binary: None,
            is_enabled: true,
            model: None,
            effort: None,
        }
    }

    /// Sets the agent's own binary.
    pub fn with_binary(mut self, binary: impl Into<PathBuf>) -> Self {
        self.binary = Some(binary.into());
        self
    }

    /// Sets whether the agent is offered for reviews.
    pub fn enabled(mut self, is_enabled: bool) -> Self {
        self.is_enabled = is_enabled;
        self
    }

    /// The command line, as Settings shows and edits it. Words containing whitespace or
    /// quotes are single-quoted so the line splits back into the same words.
    pub fn command_line(&self) -> String {
        std::iter::once(self.command.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(quote_word)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Splits a command line into command and arguments, honoring single and double quotes.
    pub fn set_command_line(&mut self, line: &str) {
        let mut words = split_words(line).into_iter();
        if let Some(command) = words.next() {
            self.command = command;
            self.args = words.collect();
        }
    }
}

fn quote_word(word: &str) -> String {
    if !word.is_empty() && !word.contains(|c: char| c.is_whitespace() || c == '\'' || c == '"') {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// Splits `line` into words the way a POSIX shell would for plain quoting.
fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    in_word = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            (None, c) => {
                current.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(current);
    }
    words
}

/// The two agents Switchfold knows how to launch out of the box.
pub fn default_agents(home: Option<&Path>) -> Vec<AgentConfig> {
    let home = home.map(Path::to_path_buf).unwrap_or_default();
    vec![
        AgentConfig::new(
            "claude-code",
            "Claude Code",
            "npx",
            vec![
                "-y".into(),
                "@agentclientprotocol/claude-agent-acp@latest".into(),
            ],
        )
        .with_binary(home.join(".local/bin/claude")),
        AgentConfig::new(
            "codex",
            "Codex",
            "npx",
            vec!["-y".into(), "@zed-industries/codex-acp@latest".into()],
        ),
        AgentConfig::new(
            "gemini-cli",
            "Gemini CLI",
            "gemini",
            vec!["--experimental-acp".into()],
        )
        .with_binary(home.join(".npm-global/bin/gemini"))
        .enabled(false),
    ]
}

/// Which revision Open in Zed opens files at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenFilesAt {
    #[default]
    PullRequestHead,
    WorkingTree,
}

impl OpenFilesAt {
    /// Every option in menu order.
    pub const ALL: [OpenFilesAt; 2] = [Self::PullRequestHead, Self::WorkingTree];

    /// The menu label.
    pub fn label(self) -> &'static str {
        match self {
            Self::PullRequestHead => "Pull request head",
            Self::WorkingTree => "Current working tree",
        }
    }
}

/// Light, dark, or following the system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    /// Every option in menu order.
    pub const ALL: [Appearance; 3] = [Self::System, Self::Light, Self::Dark];

    /// The option label.
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// The palette dark mode uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DarkTheme {
    /// Switchfold's own dark palette, from the product design's tokens.
    #[default]
    Switchfold,
    /// JetBrains' Islands Dark, as the Ghostty theme `jetbrains-islands-dark` has it.
    IslandsDark,
    /// Atom's One Dark, darkened as One Dark Pro Darker.
    OneDark,
    /// The charcoal Espresso terminal palette.
    Espresso,
    /// GitHub's dark default.
    GitHub,
}

impl DarkTheme {
    /// Every option in menu order.
    pub const ALL: [DarkTheme; 5] = [
        Self::Switchfold,
        Self::IslandsDark,
        Self::OneDark,
        Self::Espresso,
        Self::GitHub,
    ];

    /// The option label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Switchfold => "Switchfold Dark",
            Self::IslandsDark => "Islands Dark",
            Self::OneDark => "One Dark",
            Self::Espresso => "Espresso",
            Self::GitHub => "GitHub Dark",
        }
    }
}

/// The palette light mode uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightTheme {
    /// Switchfold's own light palette, from the product design's tokens.
    #[default]
    Switchfold,
    /// GitHub's light default.
    GitHub,
    /// JetBrains' Islands Light: white content on cool grey panels.
    IslandsLight,
    /// Atom's One Light.
    OneLight,
    /// Ethan Schoonover's Solarized Light.
    SolarizedLight,
}

impl LightTheme {
    /// Every option in menu order.
    pub const ALL: [LightTheme; 5] = [
        Self::Switchfold,
        Self::GitHub,
        Self::IslandsLight,
        Self::OneLight,
        Self::SolarizedLight,
    ];

    /// The option label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Switchfold => "Switchfold Light",
            Self::GitHub => "GitHub Light",
            Self::IslandsLight => "Islands Light",
            Self::OneLight => "One Light",
            Self::SolarizedLight => "Solarized Light",
        }
    }
}

/// Every persisted preference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Preferences {
    pub fetch_on_open: bool,
    pub worktree_per_pull_request: bool,
    pub remove_worktree_after_merge: bool,
    pub ask_before_commands: bool,
    pub agents_post_directly: bool,
    pub share_discussion_with_agents: bool,
    pub zed_command: String,
    pub open_files_at: OpenFilesAt,
    pub appearance: Appearance,
    /// The palette dark mode uses.
    pub dark_theme: DarkTheme,
    pub light_theme: LightTheme,
    /// Base font size in points; the whole interface scales with it.
    pub font_size: f32,
    /// Font for code, diffs, paths and hashes; `None` uses the theme's monospace font.
    pub code_font_family: Option<String>,
    /// Code size in points; `None` keeps it in proportion to the interface size.
    pub code_font_size: Option<f32>,
    pub default_agent: String,
    /// Commands the user allowed for every review in a repository, keyed by `owner/name`.
    pub always_allowed_commands: Vec<(String, String)>,
    pub triage: TriageSettings,
    pub typesafe: TypeSafeSettings,
    /// Repositories pinned to the top of the sidebar, as `owner/name`, in pin order.
    pub pinned_repositories: Vec<String>,
    /// Colors the reviewer gave repositories, as `owner/name` and color.
    pub repository_colors: Vec<(String, RepositoryColor)>,
    /// Sidebar sections the reviewer folded, by name.
    pub folded_sections: Vec<String>,
    /// The review's right sidebar is hidden, leaving a strip of check icons.
    pub inspector_collapsed: bool,
    /// Lists repositories in the sidebar under their organization or owner.
    pub group_repositories_by_owner: bool,
}

/// A color the reviewer gives a repository, to tell it apart in the sidebar and the
/// list. Each maps to a theme color, so it follows light and dark mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepositoryColor {
    Red,
    Yellow,
    Green,
    Cyan,
    Blue,
    Purple,
}

impl RepositoryColor {
    /// Every color in menu order.
    pub const ALL: [RepositoryColor; 6] = [
        Self::Red,
        Self::Yellow,
        Self::Green,
        Self::Cyan,
        Self::Blue,
        Self::Purple,
    ];

    /// The menu label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Cyan => "Cyan",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
        }
    }
}

impl Preferences {
    /// Whether `slug` is pinned.
    pub fn is_pinned(&self, slug: &str) -> bool {
        self.pinned_repositories.iter().any(|pinned| pinned == slug)
    }

    /// Pins or unpins `slug`; pinning adds it at the end.
    pub fn set_pinned(&mut self, slug: &str, pinned: bool) {
        self.pinned_repositories.retain(|existing| existing != slug);
        if pinned {
            self.pinned_repositories.push(slug.to_string());
        }
    }

    /// The color given to `slug`.
    pub fn repository_color(&self, slug: &str) -> Option<RepositoryColor> {
        self.repository_colors
            .iter()
            .find(|(repo, _)| repo == slug)
            .map(|(_, color)| *color)
    }

    /// Gives `slug` a color, or takes it away with `None`.
    pub fn set_repository_color(&mut self, slug: &str, color: Option<RepositoryColor>) {
        self.repository_colors.retain(|(repo, _)| repo != slug);
        if let Some(color) = color {
            self.repository_colors.push((slug.to_string(), color));
        }
    }

    /// Whether the sidebar section `name` is folded.
    pub fn is_folded(&self, name: &str) -> bool {
        self.folded_sections.iter().any(|folded| folded == name)
    }

    /// Folds or unfolds the sidebar section `name`.
    pub fn set_folded(&mut self, name: &str, folded: bool) {
        self.folded_sections.retain(|existing| existing != name);
        if folded {
            self.folded_sections.push(name.to_string());
        }
    }
}

/// Which triage features run, through which provider. Nothing runs until the
/// provider has a key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct TriageSettings {
    /// The provider's id, such as `typesafe`.
    pub provider: String,
    /// Rate agent findings for severity and whether they're worth acting on.
    pub findings: bool,
    /// Label review threads as blocking, suggestion, nit, question or praise.
    pub threads: bool,
    /// Rate pull requests in the inbox for kind and risk.
    pub pull_requests: bool,
}

impl Default for TriageSettings {
    fn default() -> Self {
        Self {
            provider: "typesafe".into(),
            findings: true,
            threads: true,
            pull_requests: true,
        }
    }
}

/// TypeSafe.ai settings besides the key, which lives in the Keychain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct TypeSafeSettings {
    /// The model asked, such as `jev-latest`.
    pub model: String,
}

impl Default for TypeSafeSettings {
    fn default() -> Self {
        Self {
            model: "jev-latest".into(),
        }
    }
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            fetch_on_open: true,
            worktree_per_pull_request: true,
            remove_worktree_after_merge: false,
            ask_before_commands: true,
            agents_post_directly: false,
            share_discussion_with_agents: true,
            zed_command: "/usr/local/bin/zed".into(),
            open_files_at: OpenFilesAt::PullRequestHead,
            appearance: Appearance::System,
            dark_theme: DarkTheme::Switchfold,
            light_theme: LightTheme::Switchfold,
            font_size: DEFAULT_FONT_SIZE,
            code_font_family: None,
            code_font_size: None,
            default_agent: "claude-code".into(),
            always_allowed_commands: Vec::new(),
            triage: TriageSettings::default(),
            typesafe: TypeSafeSettings::default(),
            pinned_repositories: Vec::new(),
            repository_colors: Vec::new(),
            folded_sections: Vec::new(),
            group_repositories_by_owner: false,
            inspector_collapsed: false,
        }
    }
}

/// The font size the design is drawn at.
pub const DEFAULT_FONT_SIZE: f32 = 16.;

/// Font sizes offered for interface zoom.
pub const FONT_SIZES: [f32; 5] = [14., 15., 16., 17., 18.];

/// Code font sizes offered besides matching the interface.
pub const CODE_FONT_SIZES: [f32; 9] = [10., 11., 12., 13., 14., 15., 16., 18., 20.];

impl Preferences {
    /// Whether `command` was always-allowed for `repo`.
    pub fn is_command_allowed(&self, repo: &str, command: &str) -> bool {
        self.always_allowed_commands
            .iter()
            .any(|(allowed_repo, allowed)| allowed_repo == repo && allowed == command)
    }
}

/// The complete persisted configuration.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct SettingsFile {
    pub accounts: Vec<Account>,
    pub repositories: Vec<RepositoryMapping>,
    pub agents: Vec<AgentConfig>,
    pub preferences: Preferences,
    /// Ids of the built-in agents this file has been offered, so a built-in added in a
    /// later version appears once and one the user removed stays removed.
    pub offered_agents: Vec<String>,
}

impl SettingsFile {
    /// Builds a settings file from its parts.
    pub fn new(
        accounts: Vec<Account>,
        repositories: Vec<RepositoryMapping>,
        agents: Vec<AgentConfig>,
        preferences: Preferences,
    ) -> Self {
        let offered_agents = agents.iter().map(|agent| agent.id.clone()).collect();
        Self {
            accounts,
            repositories,
            agents,
            preferences,
            offered_agents,
        }
    }

    /// Adds the built-in agents this file hasn't been offered yet. Returns whether
    /// anything changed and the file should be saved.
    pub fn offer_new_agents(&mut self, home: Option<&Path>) -> bool {
        let mut changed = false;
        for agent in default_agents(home) {
            if self.offered_agents.contains(&agent.id) {
                continue;
            }
            if !self.agents.iter().any(|known| known.id == agent.id) {
                self.agents.push(agent.clone());
            }
            self.offered_agents.push(agent.id);
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod offered_agents_tests {
    use super::*;

    #[test]
    fn new_built_in_agents_are_offered_once() {
        let mut file: SettingsFile = serde_json::from_str(
            r#"{"agents":[{"id":"claude-code","name":"Claude Code","command":"npx","args":[],"binary":null,"is_enabled":true}]}"#,
        )
        .expect("an old settings file still loads");
        assert!(file.offer_new_agents(None));
        assert!(file.agents.iter().any(|agent| agent.id == "codex"));
        file.agents.retain(|agent| agent.id != "codex");
        assert!(
            !file.offer_new_agents(None),
            "a removed agent stays removed"
        );
        assert!(!file.agents.iter().any(|agent| agent.id == "codex"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_path_abbreviates_home() {
        let home = Path::new("/Users/denis");
        assert_eq!(
            display_path(Path::new("/Users/denis/src/switchfold"), Some(home)),
            "~/src/switchfold"
        );
        assert_eq!(display_path(Path::new("/opt/x"), Some(home)), "/opt/x");
    }

    #[test]
    fn command_line_round_trips() {
        let mut agent = AgentConfig::new("a", "A", "npx", vec!["-y".into(), "pkg".into()]);
        assert_eq!(agent.command_line(), "npx -y pkg");
        agent.set_command_line("  bunx  other --acp ");
        assert_eq!(agent.command, "bunx");
        assert_eq!(agent.args, vec!["other", "--acp"]);
        agent.set_command_line("'/Users/me/My Agents/acp' --model \"big one\"");
        assert_eq!(agent.command, "/Users/me/My Agents/acp");
        assert_eq!(agent.args, vec!["--model", "big one"]);
        assert_eq!(
            agent.command_line(),
            "'/Users/me/My Agents/acp' --model 'big one'"
        );
    }

    #[test]
    fn settings_round_trip_through_json() {
        let settings = SettingsFile::new(
            vec![Account::new(
                "flaticols",
                "github.com",
                AccountSource::GhCli,
            )],
            Vec::new(),
            default_agents(None),
            Preferences::default(),
        );
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<SettingsFile>(&json).unwrap(),
            settings
        );
    }
}
