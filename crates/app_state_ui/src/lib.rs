//! State shared by Switchfold's screens: the pull request store, the settings store,
//! the merge queue, and the commands key bindings dispatch. Screens read the stores
//! through [`StoresExt::stores`] and observe them for changes.

pub mod actions;
mod merge_plan;
mod merge_queue;
mod pull_request_store;
mod settings_store;

use gpui_kit::{App, Entity, Global};

pub use merge_plan::{Activity, RowState, Skip};
pub use merge_queue::{MergeQueue, MergeQueueEvent};
pub use pull_request_store::{DetailState, ListState, PullRequestStore, PullRequestStoreEvent};
pub use settings_store::{SettingsStore, SettingsStoreEvent};

/// The application's stores, installed once as a global at startup.
pub struct Stores {
    pull_requests: Entity<PullRequestStore>,
    settings: Entity<SettingsStore>,
    merge_queue: Entity<MergeQueue>,
}

impl Global for Stores {}

impl Stores {
    /// Installs the stores as the application global.
    pub fn install(
        pull_requests: Entity<PullRequestStore>,
        settings: Entity<SettingsStore>,
        merge_queue: Entity<MergeQueue>,
        cx: &mut App,
    ) {
        cx.set_global(Self {
            pull_requests,
            settings,
            merge_queue,
        });
    }

    /// The viewer's merge queue and where each pull request in it stands.
    pub fn merge_queue(&self) -> &Entity<MergeQueue> {
        &self.merge_queue
    }

    /// Pull requests, their details and the viewer's pending review.
    pub fn pull_requests(&self) -> &Entity<PullRequestStore> {
        &self.pull_requests
    }

    /// Accounts, repositories, agents and preferences.
    pub fn settings(&self) -> &Entity<SettingsStore> {
        &self.settings
    }
}

/// Access to the installed [`Stores`].
pub trait StoresExt {
    /// The application's stores.
    fn stores(&self) -> &Stores;
}

impl StoresExt for App {
    fn stores(&self) -> &Stores {
        self.global::<Stores>()
    }
}
