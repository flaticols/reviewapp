//! TypeSafe.ai in the app: the reviewer's key from the Keychain and the model from
//! settings become the triage provider, and [`TypeSafeSection`] is where both are set.

mod section;

use std::sync::Arc;

use app_state_ui::StoresExt as _;
use gpui_kit::{App, AppContext as _, Global};
use triage::TriageProvider;
use triage_ui::TriageStoreExt as _;
use typesafe::{Client, TypeSafeProvider};

pub use section::TypeSafeSection;

/// The provider's id in settings and the Keychain.
pub const PROVIDER_ID: &str = "typesafe";

/// Where reviewers get a key.
pub const KEYS_URL: &str = "https://console.typesafe.ai/keys";

/// The key as read from the Keychain, and the key and model the triage store was last
/// given, so unrelated settings changes don't restart it.
#[derive(Default)]
struct KeyState {
    loaded: bool,
    key: Option<String>,
    applied: Option<Option<(String, String)>>,
}

impl Global for KeyState {}

/// Reads the saved key off the UI thread, then keeps the triage store's provider in
/// step with the key, the chosen provider and the model.
pub fn init(cx: &mut App) {
    cx.set_global(KeyState::default());
    let read = cx.background_spawn(async { keychain::read_api_key(PROVIDER_ID) });
    cx.spawn(async move |cx| {
        let key = read.await.ok().flatten();
        cx.update(|cx| {
            let state = cx.global_mut::<KeyState>();
            state.loaded = true;
            state.key = key;
            apply(cx);
        });
    })
    .detach();
    let settings = cx.stores().settings().clone();
    cx.observe(&settings, |_, cx| apply(cx)).detach();
}

/// The saved key, once read.
pub(crate) fn saved_key(cx: &App) -> Option<&str> {
    cx.try_global::<KeyState>()
        .and_then(|state| state.key.as_deref())
}

/// Replaces the saved key, or forgets it with `None`, and updates the provider.
pub(crate) fn set_saved_key(key: Option<String>, cx: &mut App) {
    cx.global_mut::<KeyState>().key = key;
    apply(cx);
}

fn apply(cx: &mut App) {
    let Some(state) = cx.try_global::<KeyState>() else {
        return;
    };
    if !state.loaded {
        return;
    }
    let preferences = cx.stores().settings().read(cx).preferences();
    let wanted = (preferences.triage.provider == PROVIDER_ID)
        .then(|| state.key.clone())
        .flatten()
        .map(|key| (key, preferences.typesafe.model.clone()));
    if state.applied.as_ref() == Some(&wanted) {
        return;
    }
    cx.global_mut::<KeyState>().applied = Some(wanted.clone());
    let provider = wanted.map(|(key, model)| {
        Arc::new(TypeSafeProvider::new(Client::new(key, model))) as Arc<dyn TriageProvider>
    });
    if let Some(store) = cx.triage() {
        store.update(cx, |store, cx| store.set_provider(provider, cx));
    }
}
