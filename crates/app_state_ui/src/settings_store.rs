use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use agent_registry::AgentAvailability;
use git_diff::LocalClone;
use gpui_kit::{AppContext as _, Context, EventEmitter};
use pr_model::{RepoSlug, Timestamp};
use settings_model::{
    Account, AgentConfig, CloneStatus, Preferences, RepositoryMapping, SettingsFile,
};

/// What changed in the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsStoreEvent {
    AccountsChanged,
    RepositoriesChanged,
    AgentsChanged,
    PreferencesChanged,
}

/// Owns the persisted configuration plus what was detected about the machine: clone
/// states, installed agents and the Zed launcher. Saves after every change.
pub struct SettingsStore {
    file: SettingsFile,
    path: Option<PathBuf>,
    home: Option<PathBuf>,
    clone_statuses: HashMap<String, CloneStatus>,
    agent_availability: HashMap<String, AgentAvailability>,
    zed_version: Option<String>,
}

impl EventEmitter<SettingsStoreEvent> for SettingsStore {}

impl SettingsStore {
    /// Creates the store from `file`, saving to `path`, and starts detecting agents and
    /// Zed. `clone_statuses` seeds the status of repositories, keyed by `owner/name`;
    /// any repository without one is inspected on disk.
    pub fn new(
        file: SettingsFile,
        path: Option<PathBuf>,
        home: Option<PathBuf>,
        clone_statuses: Vec<(String, CloneStatus)>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut store = Self {
            file,
            path,
            home,
            clone_statuses: clone_statuses.into_iter().collect(),
            agent_availability: HashMap::new(),
            zed_version: None,
        };
        let missing: Vec<RepositoryMapping> = store
            .file
            .repositories
            .iter()
            .filter(|mapping| !store.clone_statuses.contains_key(&mapping.slug))
            .cloned()
            .collect();
        for mapping in missing {
            let status = inspect_clone(mapping.local_path.as_deref());
            store.clone_statuses.insert(mapping.slug, status);
        }
        store.detect_tools(cx);
        store
    }

    /// Reads the settings file at `path`; `None` when it is missing or unreadable.
    pub fn read(path: &Path) -> Option<SettingsFile> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Writes `file` to `path`, creating its folder.
    pub fn write(path: &Path, file: &SettingsFile) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(file).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Copies settings saved under the app's earlier identifier to `path` when nothing
    /// is there yet, so renaming the bundle doesn't reset them.
    pub fn adopt_earlier_settings(home: &Path, path: &Path) {
        let earlier =
            home.join("Library/Application Support/dev.flaticols.switchfold-review/settings.json");
        if path.exists() || !earlier.exists() {
            return;
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::copy(earlier, path).ok();
    }

    /// The default location of the settings file.
    pub fn default_path(home: &Path) -> PathBuf {
        home.join("Library/Application Support/dev.switchfold.review/settings.json")
    }

    /// The user's home directory, for abbreviating paths.
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// Every configured account; the first is the default.
    pub fn accounts(&self) -> &[Account] {
        &self.file.accounts
    }

    /// The default account, which is the signed-in viewer.
    pub fn viewer(&self) -> Option<&Account> {
        self.file.accounts.first()
    }

    /// The signed-in person as the timeline shows them, with their profile name.
    pub fn viewer_actor(&self) -> review_model::Actor {
        match self.viewer() {
            Some(account) => {
                review_model::Actor::person(account.login.clone()).with_name(account.name.clone())
            }
            None => review_model::Actor::person("you"),
        }
    }

    /// Repository mappings in Settings order.
    pub fn repositories(&self) -> &[RepositoryMapping] {
        &self.file.repositories
    }

    /// The clone status of `slug`.
    pub fn clone_status(&self, slug: &str) -> CloneStatus {
        self.clone_statuses
            .get(slug)
            .cloned()
            .unwrap_or(CloneStatus::NotCloned)
    }

    /// The local clone of `repo`, when it is mapped to an existing folder.
    pub fn clone_path(&self, repo: &RepoSlug) -> Option<&Path> {
        let slug = repo.to_string();
        self.file
            .repositories
            .iter()
            .find(|mapping| mapping.slug == slug)
            .and_then(|mapping| mapping.local_path.as_deref())
    }

    /// Configured agents in Settings order.
    pub fn agents(&self) -> &[AgentConfig] {
        &self.file.agents
    }

    /// What is installed for agent `id`, once detected.
    pub fn agent_availability(&self, id: &str) -> Option<&AgentAvailability> {
        self.agent_availability.get(id)
    }

    /// The default agent, falling back to the first enabled one.
    pub fn default_agent(&self) -> Option<&AgentConfig> {
        let id = &self.file.preferences.default_agent;
        self.file
            .agents
            .iter()
            .find(|agent| &agent.id == id && agent.is_enabled)
            .or_else(|| self.file.agents.iter().find(|agent| agent.is_enabled))
    }

    /// The version of the Zed launcher, once detected.
    pub fn zed_version(&self) -> Option<&str> {
        self.zed_version.as_deref()
    }

    /// Every preference.
    pub fn preferences(&self) -> &Preferences {
        &self.file.preferences
    }

    /// Changes preferences and saves.
    pub fn update_preferences(
        &mut self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut Preferences),
    ) {
        let zed_before = self.file.preferences.zed_command.clone();
        update(&mut self.file.preferences);
        if self.file.preferences.zed_command != zed_before {
            self.detect_tools(cx);
        }
        self.changed(SettingsStoreEvent::PreferencesChanged, cx);
    }

    /// Adds an account, or replaces the one with the same login and host.
    pub fn add_account(&mut self, account: Account, cx: &mut Context<Self>) {
        self.file
            .accounts
            .retain(|a| !(a.login == account.login && a.host == account.host));
        self.file.accounts.push(account);
        self.changed(SettingsStoreEvent::AccountsChanged, cx);
    }

    /// Makes an account the default by moving it first.
    pub fn make_default_account(&mut self, login: &str, host: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .file
            .accounts
            .iter()
            .position(|a| a.login == login && a.host == host)
        {
            let account = self.file.accounts.remove(ix);
            self.file.accounts.insert(0, account);
            self.changed(SettingsStoreEvent::AccountsChanged, cx);
        }
    }

    /// Removes an account and deletes its stored token.
    pub fn sign_out(&mut self, login: &str, host: &str, cx: &mut Context<Self>) {
        self.file
            .accounts
            .retain(|a| !(a.login == login && a.host == host));
        let (login, host) = (login.to_string(), host.to_string());
        cx.background_spawn(async move { keychain::delete_token(&login, &host) })
            .detach();
        self.changed(SettingsStoreEvent::AccountsChanged, cx);
    }

    /// Maps the clone at `path` to the GitHub repository its `origin` points at.
    pub fn add_repository(
        &mut self,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Result<RepoSlug, String> {
        let clone = LocalClone::open(path).map_err(|error| error.to_string())?;
        let slug = clone
            .github_slug()
            .ok_or_else(|| "The clone has no GitHub remote.".to_string())?;
        let key = slug.to_string();
        self.file.repositories.retain(|mapping| mapping.slug != key);
        self.file
            .repositories
            .push(RepositoryMapping::new(&slug, Some(path.to_path_buf())));
        self.clone_statuses.insert(key, inspect_clone(Some(path)));
        self.changed(SettingsStoreEvent::RepositoriesChanged, cx);
        Ok(slug)
    }

    /// Points an existing mapping at another folder.
    pub fn set_repository_path(&mut self, slug: &str, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(mapping) = self.file.repositories.iter_mut().find(|m| m.slug == slug) {
            self.clone_statuses
                .insert(slug.to_string(), inspect_clone(Some(&path)));
            mapping.local_path = Some(path);
            self.changed(SettingsStoreEvent::RepositoriesChanged, cx);
        }
    }

    /// Removes a mapping; the clone on disk is left alone.
    pub fn remove_repository(&mut self, slug: &str, cx: &mut Context<Self>) {
        self.file
            .repositories
            .retain(|mapping| mapping.slug != slug);
        self.clone_statuses.remove(slug);
        self.changed(SettingsStoreEvent::RepositoriesChanged, cx);
    }

    /// Enables or disables an agent.
    pub fn set_agent_enabled(&mut self, id: &str, enabled: bool, cx: &mut Context<Self>) {
        if let Some(agent) = self.file.agents.iter_mut().find(|a| a.id == id) {
            agent.is_enabled = enabled;
            self.changed(SettingsStoreEvent::AgentsChanged, cx);
        }
    }

    /// Remembers the model and effort last picked for agent `id`'s reviews.
    pub fn set_agent_choices(
        &mut self,
        id: &str,
        model: Option<String>,
        effort: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(agent) = self.file.agents.iter_mut().find(|a| a.id == id)
            && (agent.model != model || agent.effort != effort)
        {
            agent.model = model;
            agent.effort = effort;
            self.changed(SettingsStoreEvent::AgentsChanged, cx);
        }
    }

    /// Makes an agent the default for reviews.
    pub fn make_default_agent(&mut self, id: &str, cx: &mut Context<Self>) {
        self.file.preferences.default_agent = id.to_string();
        self.changed(SettingsStoreEvent::AgentsChanged, cx);
    }

    /// Replaces an agent's launch command line and re-detects it.
    pub fn set_agent_command(&mut self, id: &str, command_line: &str, cx: &mut Context<Self>) {
        if let Some(agent) = self.file.agents.iter_mut().find(|a| a.id == id) {
            agent.set_command_line(command_line);
            self.detect_tools(cx);
            self.changed(SettingsStoreEvent::AgentsChanged, cx);
        }
    }

    /// Adds a custom ACP agent launched by `command_line`.
    pub fn add_agent(&mut self, name: &str, command_line: &str, cx: &mut Context<Self>) {
        let id = format!("custom-{}", self.file.agents.len() + 1);
        let mut agent = AgentConfig::new(id, name, "", Vec::new());
        agent.set_command_line(command_line);
        self.file.agents.push(agent);
        self.detect_tools(cx);
        self.changed(SettingsStoreEvent::AgentsChanged, cx);
    }

    /// Removes a custom agent.
    pub fn remove_agent(&mut self, id: &str, cx: &mut Context<Self>) {
        self.file.agents.retain(|agent| agent.id != id);
        self.changed(SettingsStoreEvent::AgentsChanged, cx);
    }

    fn detect_tools(&mut self, cx: &mut Context<Self>) {
        let agents = self.file.agents.clone();
        let zed = self.file.preferences.zed_command.clone();
        cx.spawn(async move |this, cx| {
            let (availability, zed_version) = cx
                .background_spawn(async move {
                    let availability: HashMap<String, AgentAvailability> = agents
                        .iter()
                        .map(|agent| (agent.id.clone(), agent_registry::detect(agent)))
                        .collect();
                    (availability, editor_launch::zed_version(&zed))
                })
                .await;
            this.update(cx, |this, cx| {
                this.agent_availability = availability;
                this.zed_version = zed_version;
                cx.emit(SettingsStoreEvent::AgentsChanged);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn changed(&mut self, event: SettingsStoreEvent, cx: &mut Context<Self>) {
        if let Some(path) = self.path.clone() {
            let file = self.file.clone();
            cx.background_spawn(async move {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).ok();
                }
                if let Ok(json) = serde_json::to_string_pretty(&file) {
                    std::fs::write(path, json).ok();
                }
            })
            .detach();
        }
        cx.emit(event);
        cx.notify();
    }
}

/// The status of a clone at `path` as found on disk.
fn inspect_clone(path: Option<&Path>) -> CloneStatus {
    let Some(path) = path else {
        return CloneStatus::NotCloned;
    };
    if !path.is_dir() || LocalClone::open(path).is_err() {
        return CloneStatus::FolderNotFound;
    }
    let git_dir = path.join(".git");
    let fetched_at = std::fs::metadata(git_dir.join("FETCH_HEAD"))
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|elapsed| Timestamp::from_unix(elapsed.as_secs() as i64))
        .unwrap_or_else(Timestamp::now);
    let worktrees = std::fs::read_dir(git_dir.join("worktrees"))
        .map(|entries| entries.count() as u32)
        .unwrap_or(0);
    CloneStatus::Fetched {
        at: fetched_at,
        worktrees,
    }
}
