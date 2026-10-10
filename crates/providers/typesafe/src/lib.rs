//! TypeSafe.ai as a triage provider, with the reviewer's own API key: a client for its
//! System One endpoint, and [`TypeSafeProvider`], which asks it the questions the
//! `triage` crate defines.
//!
//! System One answers typed questions about a `state` with calibrated probabilities:
//! a Noul is a yes/no probability, a Choice picks one option, a Score places the state
//! on a rubric. See <https://docs.typesafe.ai/api>.

mod client;
mod provider;
mod question;

pub use client::{Client, DEFAULT_MODEL, ModelInfo, TypeSafeError};
pub use provider::TypeSafeProvider;
pub use question::{Answer, Question};
