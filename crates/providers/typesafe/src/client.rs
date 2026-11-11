use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::question::{Answer, Question, Request, Response};

/// The model Switchfold asks unless the reviewer picks another.
pub const DEFAULT_MODEL: &str = "jev-latest";

const BASE_URL: &str = "https://api.typesafe.ai";

/// How many times a rate-limited request is retried before giving up.
const RATE_LIMIT_RETRIES: u32 = 3;

/// Why a request failed, in words for the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeSafeError {
    /// The key was refused.
    Unauthorized,
    /// Still rate limited after retrying.
    RateLimited,
    /// Anything else the API or the network reported.
    Other(String),
}

impl fmt::Display for TypeSafeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => f.write_str("TypeSafe.ai refused the API key."),
            Self::RateLimited => {
                f.write_str("TypeSafe.ai is rate limiting this key; try again shortly.")
            }
            Self::Other(message) => write!(f, "TypeSafe.ai: {message}"),
        }
    }
}

impl std::error::Error for TypeSafeError {}

/// A model the key may use.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct ModelInfo {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// The System One API with one key and model. Calls block; make them off the UI thread.
#[derive(Clone)]
pub struct Client {
    key: String,
    model: String,
    base_url: String,
    agent: ureq::Agent,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// A client for `key`, asking `model`.
    pub fn new(key: impl Into<String>, model: impl Into<String>) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into();
        Self {
            key: key.into(),
            model: model.into(),
            base_url: BASE_URL.into(),
            agent,
        }
    }

    /// Points the client at another server, for tests.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// The model this client asks.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The models the key may use. Doubles as a check that the key works.
    pub fn models(&self) -> Result<Vec<ModelInfo>, TypeSafeError> {
        #[derive(Deserialize)]
        struct Models {
            #[serde(default)]
            models: Vec<ModelInfo>,
            #[serde(default)]
            data: Vec<ModelInfo>,
        }
        let body = self.send(|| {
            self.agent
                .get(format!("{}/v1/models", self.base_url))
                .header("Authorization", &format!("Bearer {}", self.key))
                .call()
        })?;
        let parsed: Models = serde_json::from_str(&body).map_err(|error| {
            TypeSafeError::Other(format!("unexpected models response: {error}"))
        })?;
        Ok(if parsed.models.is_empty() {
            parsed.data
        } else {
            parsed.models
        })
    }

    /// Asks every question in `questions` about `state`. Returns the answers under the
    /// questions' ids, and the versioned model that answered.
    pub fn system_one(
        &self,
        state: &Value,
        questions: &BTreeMap<String, Question>,
    ) -> Result<(String, BTreeMap<String, Answer>), TypeSafeError> {
        let request = Request {
            state,
            model: &self.model,
            questions,
        };
        let body = self.send(|| {
            self.agent
                .post(format!("{}/v1/systemone", self.base_url))
                .header("Authorization", &format!("Bearer {}", self.key))
                .send_json(&request)
        })?;
        let response: Response = serde_json::from_str(&body)
            .map_err(|error| TypeSafeError::Other(format!("unexpected response: {error}")))?;
        Ok((response.model, response.answers))
    }

    /// Sends a request, retrying while rate limited, and returns the body of a success.
    fn send(
        &self,
        request: impl Fn() -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<String, TypeSafeError> {
        for attempt in 0..=RATE_LIMIT_RETRIES {
            let mut response =
                request().map_err(|error| TypeSafeError::Other(error.to_string()))?;
            let status = response.status().as_u16();
            if status == 429 && attempt < RATE_LIMIT_RETRIES {
                let wait = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.trim().parse::<u64>().ok())
                    .unwrap_or(1 << attempt)
                    .min(30);
                std::thread::sleep(Duration::from_secs(wait));
                continue;
            }
            let body = response
                .body_mut()
                .read_to_string()
                .map_err(|error| TypeSafeError::Other(error.to_string()))?;
            return match status {
                200..=299 => Ok(body),
                401 | 403 => Err(TypeSafeError::Unauthorized),
                429 => Err(TypeSafeError::RateLimited),
                _ => Err(TypeSafeError::Other(format!(
                    "HTTP {status}: {}",
                    error_message(&body)
                ))),
            };
        }
        Err(TypeSafeError::RateLimited)
    }
}

/// The message in an error body, which may be `{"error": {"message": …}}`,
/// `{"message": …}` or plain text.
fn error_message(body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    parsed
        .as_ref()
        .and_then(|value| {
            value["error"]["message"]
                .as_str()
                .or_else(|| value["error"].as_str())
                .or_else(|| value["message"].as_str())
                .or_else(|| value["detail"].as_str())
        })
        .map(str::to_string)
        .unwrap_or_else(|| body.chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_bodies_read_as_their_message() {
        assert_eq!(
            error_message(r#"{"error":{"message":"bad criteria"}}"#),
            "bad criteria"
        );
        assert_eq!(error_message(r#"{"message":"nope"}"#), "nope");
        assert_eq!(error_message("plain"), "plain");
    }
}
