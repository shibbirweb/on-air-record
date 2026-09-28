//! Timeline bookmarks.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::{Extension, Json};

use crate::app::AppState;
use crate::controllers::auth_context::{Caller, RequestOrigin};
use crate::dto::{BookmarkDto, BookmarkListResponse, CreateBookmarkRequest, UpdateBookmarkRequest};
use crate::error::AppResult;
use crate::models::activity::ActivityEvent;
use crate::models::{BookmarkDraft, BookmarkPatch};

/// `GET /api/bookmarks`
pub async fn list(State(state): State<Arc<AppState>>) -> AppResult<Json<BookmarkListResponse>> {
    let bookmarks = state
        .bookmarks
        .list()?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(Json(BookmarkListResponse { bookmarks }))
}

/// `POST /api/bookmarks`
pub async fn create(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    RequestOrigin(origin): RequestOrigin,
    Json(request): Json<CreateBookmarkRequest>,
) -> AppResult<Json<BookmarkDto>> {
    let draft: BookmarkDraft = request.into();
    let bookmark = state.bookmarks.create(draft)?;
    state.activity.record(
        caller.actor(),
        &origin,
        ActivityEvent::BookmarkAdded {
            label: bookmark.label.clone(),
            timestamp_ms: bookmark.timestamp_ms,
        },
    );
    Ok(Json(bookmark.into()))
}

/// `PATCH /api/bookmarks/:id`
pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(bookmark_id): Path<i64>,
    Json(request): Json<UpdateBookmarkRequest>,
) -> AppResult<Json<BookmarkDto>> {
    let patch: BookmarkPatch = request.into();
    Ok(Json(state.bookmarks.update(bookmark_id, &patch)?.into()))
}

/// `DELETE /api/bookmarks/:id`
pub async fn remove(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    RequestOrigin(origin): RequestOrigin,
    Path(bookmark_id): Path<i64>,
) -> AppResult<Json<BookmarkListResponse>> {
    // Read first, so the entry can say which one went.
    let label = state
        .bookmarks
        .list()?
        .into_iter()
        .find(|bookmark| bookmark.id == bookmark_id)
        .map(|bookmark| bookmark.label);
    state.bookmarks.delete(bookmark_id)?;
    if let Some(label) = label {
        state.activity.record(
            caller.actor(),
            &origin,
            ActivityEvent::BookmarkRemoved { label },
        );
    }

    // Returning the remaining list saves the client a follow up request purely to refresh the timeline.
    let bookmarks = state
        .bookmarks
        .list()?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(Json(BookmarkListResponse { bookmarks }))
}
