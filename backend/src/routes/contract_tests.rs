//! The router's half of the request contract: every call the frontend makes, as written down in
//! `contracts/requests.json` (which the frontend's own tests hold its `api` client to), reaches a handler.
//!
//! `contract_tests.rs` at the crate root proves each body and query parses into the right type. What it
//! cannot see is the method and path: the frontend could call `PUT /api/bookmark/7` with a perfect body and
//! only the browser would notice. Here each request goes through the real router, signed in as an admin,
//! and its answer must come from one of our handlers: a success, or our JSON error envelope for a refusal
//! the handler decided on (a bookmark that does not exist, a wrong password). Never the "no such endpoint"
//! 404, a 405 for the wrong method, or one of axum's plain text rejections for a body that did not fit.

use axum::http::{Method, StatusCode};
use serde_json::Value;

use super::test_support::{app, call_from, set_up_admin, SAME_ORIGIN};

const REQUESTS: &str = include_str!("../../../contracts/requests.json");

/// Every request in the fixture as method, path with its query, and body.
fn requests() -> Vec<(String, Method, String, Option<Value>)> {
    let fixture: Value = serde_json::from_str(REQUESTS).expect("requests.json is JSON");
    let mut all = Vec::new();

    for (name, examples) in fixture["bodies"].as_object().expect("bodies") {
        for example in examples.as_array().expect("examples") {
            let method = example["method"].as_str().expect("method");
            all.push((
                name.clone(),
                Method::from_bytes(method.as_bytes()).expect("a method"),
                example["path"].as_str().expect("path").to_string(),
                Some(example["body"].clone()),
            ));
        }
    }
    for (name, examples) in fixture["queries"].as_object().expect("queries") {
        for example in examples.as_array().expect("examples") {
            let query = example["query"]
                .as_object()
                .expect("query")
                .iter()
                .map(|(key, value)| match value {
                    Value::String(text) => format!("{key}={text}"),
                    other => format!("{key}={other}"),
                })
                .collect::<Vec<_>>()
                .join("&");
            let path = example["path"].as_str().expect("path");
            all.push((name.clone(), Method::GET, format!("{path}?{query}"), None));
        }
    }
    all
}

#[tokio::test]
async fn every_request_the_frontend_makes_reaches_a_handler() {
    let app = app("contract-routes");
    let admin = set_up_admin(&app).await;
    let requests = requests();
    assert!(
        requests.len() >= 20,
        "the fixture lists the frontend's calls"
    );

    for (name, method, path, body) in requests {
        let reply = call_from(
            &app,
            method.clone(),
            &format!("/api{path}"),
            Some(&admin),
            body,
            Some(SAME_ORIGIN),
        )
        .await;
        let what = format!("{name}: {method} /api{path} answered {}", reply.status);

        assert_ne!(
            reply.status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{what}, the wrong method"
        );
        let message = reply.body["error"]["message"].as_str().unwrap_or("");
        let bare_path = path.split('?').next().unwrap_or(&path);
        assert_ne!(
            message,
            format!("there is no {method} /api{bare_path} in this API"),
            "{what}, no such endpoint"
        );
        if !reply.status.is_success() {
            assert!(
                reply.body["error"]["code"].is_string(),
                "{what} without our error envelope, so the request never reached the handler: {}",
                reply.body
            );
        }
    }
}
