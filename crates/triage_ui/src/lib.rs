//! Triage in the app, whichever provider answers: [`TriageStore`] watches the pull
//! request list and each loaded pull request, asks the provider about what is new or
//! changed, and keeps the answers so the list and review screens can show them.

mod inputs;
mod store;

pub use store::{TriageStore, TriageStoreExt};
