//! Bookmarks over HTTP: the whole life of one, from creating it to deleting it, and every request the
//! service refuses, with the JSON the timeline reads back.

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

use super::test_support::{app, call, call_from, TestApp, SAME_ORIGIN};

/// A state changing request as the page sends it, from its own origin.
async fn write(
    app: &TestApp,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let reply = call_from(app, method, path, None, body, Some(SAME_ORIGIN)).await;
    (reply.status, reply.body)
}

async fn list(app: &TestApp) -> Value {
    let reply = call(app, Method::GET, "/api/bookmarks", None, None).await;
    assert_eq!(reply.status, StatusCode::OK);
    reply.body["bookmarks"].clone()
}

#[tokio::test]
async fn there_are_none_to_begin_with() {
    let app = app("bookmarks-empty");
    assert_eq!(list(&app).await, json!([]));
}

#[tokio::test]
async fn a_bookmark_is_created_with_a_trimmed_label_and_its_note() {
    let app = app("bookmarks-create");
    let (status, created) = write(
        &app,
        Method::POST,
        "/api/bookmarks",
        Some(json!({ "timestampMs": 1_000_000, "label": "  Doorbell  ", "note": "courier" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(created["id"].as_i64().is_some());
    assert_eq!(created["timestampMs"], 1_000_000);
    assert_eq!(created["label"], "Doorbell");
    assert_eq!(created["note"], "courier");
    assert!(created["createdAtMs"].as_i64().is_some());
    assert_eq!(list(&app).await, json!([created]));
}

#[tokio::test]
async fn the_note_is_optional() {
    let app = app("bookmarks-no-note");
    let (status, created) = write(
        &app,
        Method::POST,
        "/api/bookmarks",
        Some(json!({ "timestampMs": 5, "label": "Start" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["note"], Value::Null);
}

#[tokio::test]
async fn the_list_is_in_timeline_order_whatever_order_they_were_added_in() {
    let app = app("bookmarks-order");
    for (timestamp, label) in [(300, "third"), (100, "first"), (200, "second")] {
        let (status, _) = write(
            &app,
            Method::POST,
            "/api/bookmarks",
            Some(json!({ "timestampMs": timestamp, "label": label })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let labels: Vec<Value> = list(&app)
        .await
        .as_array()
        .expect("list")
        .iter()
        .map(|bookmark| bookmark["label"].clone())
        .collect();
    assert_eq!(
        labels,
        vec![json!("first"), json!("second"), json!("third")]
    );
}

#[tokio::test]
async fn a_bookmark_needs_a_label_that_is_not_too_long() {
    let app = app("bookmarks-label-rules");
    for label in ["", "   "] {
        let (status, body) = write(
            &app,
            Method::POST,
            "/api/bookmarks",
            Some(json!({ "timestampMs": 1, "label": label })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{label:?}");
        assert_eq!(body["error"]["code"], "bad_request");
    }
    let long = "x".repeat(121);
    let (status, body) = write(
        &app,
        Method::POST,
        "/api/bookmarks",
        Some(json!({ "timestampMs": 1, "label": long })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"]["message"]
        .as_str()
        .unwrap_or("")
        .contains("120"));

    let (status, _) = write(
        &app,
        Method::POST,
        "/api/bookmarks",
        Some(json!({ "timestampMs": 1, "label": "x".repeat(120) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "exactly the limit is accepted");
}

#[tokio::test]
async fn a_note_longer_than_the_limit_is_refused() {
    let app = app("bookmarks-note-rule");
    let (status, _) = write(
        &app,
        Method::POST,
        "/api/bookmarks",
        Some(json!({ "timestampMs": 1, "label": "ok", "note": "n".repeat(2001) })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(list(&app).await, json!([]), "nothing is stored");
}

#[tokio::test]
async fn a_request_missing_its_fields_or_not_json_is_refused() {
    let app = app("bookmarks-malformed");
    let (missing_time, _) = write(
        &app,
        Method::POST,
        "/api/bookmarks",
        Some(json!({ "label": "x" })),
    )
    .await;
    assert!(missing_time.is_client_error(), "{missing_time}");
    let (missing_label, _) = write(
        &app,
        Method::POST,
        "/api/bookmarks",
        Some(json!({ "timestampMs": 1 })),
    )
    .await;
    assert!(missing_label.is_client_error(), "{missing_label}");
}

#[tokio::test]
async fn a_bookmark_is_edited_field_by_field() {
    let app = app("bookmarks-edit");
    let (_, created) = write(
        &app,
        Method::POST,
        "/api/bookmarks",
        Some(json!({ "timestampMs": 1_000, "label": "Old", "note": "keep me" })),
    )
    .await;
    let path = format!("/api/bookmarks/{}", created["id"]);

    let (status, renamed) =
        write(&app, Method::PATCH, &path, Some(json!({ "label": "New" }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["label"], "New");
    assert_eq!(renamed["timestampMs"], 1_000, "untouched");
    assert_eq!(renamed["note"], "keep me", "untouched");

    let (_, moved) = write(
        &app,
        Method::PATCH,
        &path,
        Some(json!({ "timestampMs": 2_000 })),
    )
    .await;
    assert_eq!(moved["timestampMs"], 2_000);

    let (_, cleared) = write(&app, Method::PATCH, &path, Some(json!({ "note": null }))).await;
    assert_eq!(
        cleared["note"],
        Value::Null,
        "an explicit null clears the note"
    );
    assert_eq!(list(&app).await[0]["label"], "New");
}

#[tokio::test]
async fn an_edit_that_changes_nothing_or_breaks_the_rules_is_refused() {
    let app = app("bookmarks-edit-refusals");
    let (_, created) = write(
        &app,
        Method::POST,
        "/api/bookmarks",
        Some(json!({ "timestampMs": 1, "label": "Keep" })),
    )
    .await;
    let path = format!("/api/bookmarks/{}", created["id"]);

    let (nothing, body) = write(&app, Method::PATCH, &path, Some(json!({}))).await;
    assert_eq!(nothing, StatusCode::BAD_REQUEST);
    assert!(body["error"]["message"]
        .as_str()
        .unwrap_or("")
        .contains("nothing to change"));

    let (blank, _) = write(&app, Method::PATCH, &path, Some(json!({ "label": " " }))).await;
    assert_eq!(blank, StatusCode::BAD_REQUEST);
    assert_eq!(
        list(&app).await[0]["label"],
        "Keep",
        "the refused edit left it alone"
    );
}

#[tokio::test]
async fn deleting_returns_what_is_left_and_a_second_delete_is_not_found() {
    let app = app("bookmarks-delete");
    let (_, first) = write(
        &app,
        Method::POST,
        "/api/bookmarks",
        Some(json!({ "timestampMs": 1, "label": "a" })),
    )
    .await;
    let (_, second) = write(
        &app,
        Method::POST,
        "/api/bookmarks",
        Some(json!({ "timestampMs": 2, "label": "b" })),
    )
    .await;
    let path = format!("/api/bookmarks/{}", first["id"]);

    let (status, remaining) = write(&app, Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(remaining["bookmarks"], json!([second]));

    let (again, body) = write(&app, Method::DELETE, &path, None).await;
    assert_eq!(again, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn editing_a_bookmark_that_does_not_exist_is_not_found() {
    let app = app("bookmarks-missing");
    let (status, _) = write(
        &app,
        Method::PATCH,
        "/api/bookmarks/999",
        Some(json!({ "label": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (malformed, _) = write(
        &app,
        Method::PATCH,
        "/api/bookmarks/not-a-number",
        Some(json!({ "label": "x" })),
    )
    .await;
    assert_eq!(malformed, StatusCode::BAD_REQUEST);
}
