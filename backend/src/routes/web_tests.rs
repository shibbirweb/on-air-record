//! What the service answers outside the API routes: the web interface for every page path, so a reloaded
//! or bookmarked `/settings` still loads, and a JSON 404 for any unknown path under `/api`, so a mistyped
//! or retired endpoint fails as one rather than as a page of HTML.

use axum::http::header::CONTENT_TYPE;
use axum::http::{Method, StatusCode};
use serde_json::Value;

use super::test_support::{
    app, app_serving_ui, call, call_from, get_raw, set_up_admin, SAME_ORIGIN,
};

#[tokio::test]
async fn the_web_interface_is_served_at_the_root_and_at_every_page_path() {
    let app = app_serving_ui("web-pages");
    for path in [
        "/",
        "/settings",
        "/account",
        "/some/page/that/does/not/exist",
    ] {
        let reply = get_raw(&app, path).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        let content_type = reply.headers[CONTENT_TYPE].to_str().expect("ascii");
        assert!(
            content_type.starts_with("text/html"),
            "{path}: {content_type}"
        );
        assert!(
            String::from_utf8_lossy(&reply.bytes).contains("the test interface"),
            "{path} is the interface's own page",
        );
    }
}

#[tokio::test]
async fn a_file_of_the_interface_is_served_as_itself_not_as_the_page() {
    let app = app_serving_ui("web-assets");
    let reply = get_raw(&app, "/assets/app.js").await;
    assert_eq!(reply.status, StatusCode::OK);
    let content_type = reply.headers[CONTENT_TYPE].to_str().expect("ascii");
    assert!(content_type.contains("javascript"), "{content_type}");
    assert_eq!(reply.bytes, b"console.log('app');");
}

#[tokio::test]
async fn an_unknown_api_path_is_a_json_not_found_not_the_web_page() {
    let app = app("web-unknown-api");
    for path in ["/api/nope", "/api/timeline/nope", "/api/bookmarks/1/extra"] {
        let reply = get_raw(&app, path).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
        let content_type = reply.headers[CONTENT_TYPE].to_str().expect("ascii");
        assert!(
            content_type.starts_with("application/json"),
            "{path}: {content_type}"
        );
        let body: Value = serde_json::from_slice(&reply.bytes).expect("JSON");
        assert_eq!(body["error"]["code"], "not_found", "{path}");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains(path),
            "the message names the path: {body}",
        );
    }
}

#[tokio::test]
async fn an_unknown_api_path_is_not_found_for_every_method_and_whether_or_not_signed_in() {
    let app = app("web-unknown-api-accounts");
    let admin = set_up_admin(&app).await;
    let signed_out = call(&app, Method::GET, "/api/nope", None, None).await;
    assert_eq!(signed_out.status, StatusCode::NOT_FOUND);
    let signed_in = call(&app, Method::GET, "/api/nope", Some(&admin), None).await;
    assert_eq!(signed_in.status, StatusCode::NOT_FOUND);
    let posted = call_from(
        &app,
        Method::POST,
        "/api/nope",
        Some(&admin),
        None,
        Some(SAME_ORIGIN),
    )
    .await;
    assert_eq!(posted.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_known_path_with_the_wrong_method_says_so_rather_than_not_found() {
    let app = app("web-wrong-method");
    let reply = call_from(
        &app,
        Method::DELETE,
        "/api/health",
        None,
        None,
        Some(SAME_ORIGIN),
    )
    .await;
    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
}
