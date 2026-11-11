use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One typed question, as the API takes it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Yes or no; answered with the probability of yes.
    Noul {
        instructions: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    /// One option from `criteria`, keyed by option, valued by its description.
    Choice {
        instructions: String,
        criteria: BTreeMap<String, String>,
    },
    /// A level on the ordered rubric `criteria`, lowest first.
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

/// What a yes and a no mean for a [`Question::Noul`].
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub yes: String,
    #[serde(rename = "false")]
    pub no: String,
}

impl Question {
    /// A yes/no question; `yes` and `no` say what each answer means.
    pub fn noul(instructions: &str, yes: &str, no: &str) -> Self {
        Self::Noul {
            instructions: instructions.into(),
            criteria: Some(NoulCriteria {
                yes: yes.into(),
                no: no.into(),
            }),
        }
    }

    /// A choice among `options`, each an option key and its description.
    pub fn choice(instructions: &str, options: &[(&str, &str)]) -> Self {
        Self::Choice {
            instructions: instructions.into(),
            criteria: options
                .iter()
                .map(|(key, description)| (key.to_string(), description.to_string()))
                .collect(),
        }
    }

    /// A score on `levels`, lowest first.
    pub fn score(instructions: &str, levels: &[&str]) -> Self {
        Self::Score {
            instructions: instructions.into(),
            criteria: levels.iter().map(|level| level.to_string()).collect(),
        }
    }
}

/// One answer, under the id its question was sent with.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Noul {
        /// The probability the answer is yes.
        noul: f32,
    },
    Choice {
        choice: String,
        confidence: f32,
        #[serde(default)]
        probabilities: BTreeMap<String, f32>,
    },
    Score {
        /// The probability-weighted level, from 0 to the number of levels minus one.
        score: f32,
        confidence: f32,
    },
}

impl Answer {
    /// The probability of yes, for a Noul.
    pub fn yes(&self) -> Option<f32> {
        match self {
            Self::Noul { noul } => Some(*noul),
            _ => None,
        }
    }

    /// The chosen option and its confidence, for a Choice.
    pub fn chosen(&self) -> Option<(&str, f32)> {
        match self {
            Self::Choice {
                choice, confidence, ..
            } => Some((choice, *confidence)),
            _ => None,
        }
    }

    /// The score and its confidence, for a Score.
    pub fn scored(&self) -> Option<(f32, f32)> {
        match self {
            Self::Score { score, confidence } => Some((*score, *confidence)),
            _ => None,
        }
    }
}

/// The request body.
#[derive(Serialize)]
pub(crate) struct Request<'a> {
    pub state: &'a Value,
    pub model: &'a str,
    pub questions: &'a BTreeMap<String, Question>,
}

/// The response body.
#[derive(Deserialize)]
pub(crate) struct Response {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn questions_and_answers_match_the_documented_shapes() {
        let questions: BTreeMap<String, Question> = [
            (
                "urgent".to_string(),
                Question::noul("Urgent?", "Time-sensitive", "No rush"),
            ),
            (
                "team".to_string(),
                Question::choice("Which team?", &[("billing", "Payments")]),
            ),
            (
                "anger".to_string(),
                Question::score("How angry?", &["Calm", "Furious"]),
            ),
        ]
        .into();
        assert_eq!(
            serde_json::to_value(&questions).unwrap(),
            json!({
                "urgent": {"type": "noul", "instructions": "Urgent?", "criteria": {"true": "Time-sensitive", "false": "No rush"}},
                "team": {"type": "choice", "instructions": "Which team?", "criteria": {"billing": "Payments"}},
                "anger": {"type": "score", "instructions": "How angry?", "criteria": ["Calm", "Furious"]}
            })
        );
        let response: Response = serde_json::from_value(json!({
            "model": "jev-1.13.0",
            "answers": {
                "department": {"type": "choice", "choice": "technical", "confidence": 0.78, "probabilities": {"technical": 0.85, "billing": 0.15}},
                "frustration": {"type": "score", "score": 1.0, "confidence": 1.0, "legend": {"0": "Calm"}, "probabilities": {"0": 0.0}},
                "is_urgent": {"type": "noul", "noul": 1.0}
            },
            "usage": {"input_tokens": 392, "output_tokens": 65}
        }))
        .unwrap();
        assert_eq!(
            response.answers["department"].chosen(),
            Some(("technical", 0.78))
        );
        assert_eq!(response.answers["frustration"].scored(), Some((1.0, 1.0)));
        assert_eq!(response.answers["is_urgent"].yes(), Some(1.0));
    }
}
