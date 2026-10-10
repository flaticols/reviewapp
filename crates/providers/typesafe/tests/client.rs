//! The client against a local server that plays TypeSafe.ai's documented API.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use serde_json::{Value, json};
use triage::{FindingInput, Severity, TriageProvider};
use typesafe::{Client, TypeSafeError, TypeSafeProvider};

/// A request as the server saw it.
struct Seen {
    request_line: String,
    authorization: String,
    body: Value,
}

/// A canned response: status, extra headers, body.
type Reply = (u16, Vec<(&'static str, &'static str)>, String);

/// Serves `responses` in order, one per connection, and reports each request.
fn serve(responses: Vec<Reply>) -> (String, mpsc::Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for (status, headers, body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut length = 0;
            let mut authorization = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if let Some(value) = lower.strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                if lower.starts_with("authorization:") {
                    authorization = line["authorization:".len()..].trim().to_string();
                }
            }
            let mut buffer = vec![0; length];
            reader.read_exact(&mut buffer).unwrap();
            tx.send(Seen {
                request_line: request_line.trim_end().to_string(),
                authorization,
                body: serde_json::from_slice(&buffer).unwrap_or(Value::Null),
            })
            .unwrap();
            let extra: String = headers
                .iter()
                .map(|(k, v)| format!("{k}: {v}\r\n"))
                .collect();
            write!(
                stream,
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n{extra}\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    (base, rx)
}

#[test]
fn asks_a_finding_in_one_request_and_reads_the_answers() {
    let answer = json!({
        "model": "jev-1.13.0",
        "answers": {
            "severity": {"type": "score", "score": 3.2, "confidence": 0.9, "legend": {}, "probabilities": {}},
            "actionable": {"type": "noul", "noul": 0.92},
            "category": {"type": "choice", "choice": "bug", "confidence": 0.8, "probabilities": {"bug": 0.8}}
        },
        "usage": {"input_tokens": 300, "output_tokens": 20}
    });
    let (base, seen) = serve(vec![(200, vec![], answer.to_string())]);
    let provider = TypeSafeProvider::new(Client::new("ts-key", "jev-latest").with_base_url(base));
    let finding = FindingInput::new("src/a.rs", 3, "Race on `hunks`.", vec!["+x".into()]);
    let triage = provider.triage_finding(&finding).unwrap();
    assert_eq!(triage.severity, Severity::Major);
    assert!(triage.actionable > 0.9);
    assert_eq!(triage.category, Some(triage::FindingCategory::Bug));

    let request = seen.recv().unwrap();
    assert_eq!(request.request_line, "POST /v1/systemone HTTP/1.1");
    assert_eq!(request.authorization, "Bearer ts-key");
    assert_eq!(request.body["model"], "jev-latest");
    assert_eq!(request.body["state"]["file"], "src/a.rs");
    assert_eq!(request.body["questions"]["severity"]["type"], "score");
    assert_eq!(
        request.body["questions"]["severity"]["criteria"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(request.body["questions"]["actionable"]["type"], "noul");
    assert!(request.body["questions"]["category"]["criteria"]["security"].is_string());
}

#[test]
fn a_refused_key_and_rate_limits_read_plainly() {
    let (base, _seen) = serve(vec![(
        401,
        vec![],
        r#"{"error":{"message":"invalid key"}}"#.into(),
    )]);
    let models = Client::new("bad", "jev-latest")
        .with_base_url(base)
        .models();
    assert_eq!(models, Err(TypeSafeError::Unauthorized));

    let (base, seen) = serve(vec![
        (429, vec![("retry-after", "0")], "{}".into()),
        (
            200,
            vec![],
            r#"{"models":[{"name":"jev-latest","description":"Flagship"}]}"#.into(),
        ),
    ]);
    let models = Client::new("ts-key", "jev-latest")
        .with_base_url(base)
        .models()
        .unwrap();
    assert_eq!(models[0].name, "jev-latest");
    assert_eq!(seen.iter().take(2).count(), 2, "retried once after the 429");
}

/// Against the real API: `TYPESAFE_API_KEY=… cargo test -p typesafe -- --ignored live`.
#[test]
#[ignore = "calls api.typesafe.ai with a real key"]
fn live_finding() {
    let key = std::env::var("TYPESAFE_API_KEY").expect("TYPESAFE_API_KEY");
    let provider = TypeSafeProvider::new(Client::new(key, typesafe::DEFAULT_MODEL));
    let finding = FindingInput::new(
        "src/auth.rs",
        42,
        "The session token is compared with `==`, which leaks timing; use a constant-time comparison.",
        vec![
            " fn check(token: &str) -> bool {".into(),
            "+    token == SECRET".into(),
            " }".into(),
        ],
    );
    let triage = provider.triage_finding(&finding).unwrap();
    eprintln!("{triage:?}");
    assert!(triage.severity >= Severity::Moderate);
}
