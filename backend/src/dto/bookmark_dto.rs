//! Bookmark payloads.

use serde::{Deserialize, Deserializer, Serialize};

use crate::models::{Bookmark, BookmarkDraft, BookmarkPatch};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkDto {
    pub id: i64,
    pub timestamp_ms: i64,
    pub label: String,
    pub note: Option<String>,
    pub created_at_ms: i64,
}

impl From<Bookmark> for BookmarkDto {
    fn from(bookmark: Bookmark) -> Self {
        Self {
            id: bookmark.id,
            timestamp_ms: bookmark.timestamp_ms,
            label: bookmark.label,
            note: bookmark.note,
            created_at_ms: bookmark.created_at_ms,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkListResponse {
    pub bookmarks: Vec<BookmarkDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBookmarkRequest {
    pub timestamp_ms: i64,
    pub label: String,
    #[serde(default)]
    pub note: Option<String>,
}

impl From<CreateBookmarkRequest> for BookmarkDraft {
    fn from(request: CreateBookmarkRequest) -> Self {
        Self {
            timestamp_ms: request.timestamp_ms,
            label: request.label,
            note: request.note,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBookmarkRequest {
    #[serde(default)]
    pub timestamp_ms: Option<i64>,
    #[serde(default)]
    pub label: Option<String>,
    /// Present and null clears the note, absent leaves it alone.
    #[serde(default, deserialize_with = "deserialize_nested_option")]
    pub note: Option<Option<String>>,
}

impl From<UpdateBookmarkRequest> for BookmarkPatch {
    fn from(request: UpdateBookmarkRequest) -> Self {
        Self {
            timestamp_ms: request.timestamp_ms,
            label: request.label,
            note: request.note,
        }
    }
}

fn deserialize_nested_option<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_create_request_needs_only_a_time_and_a_label() {
        let request: CreateBookmarkRequest =
            serde_json::from_str(r#"{"timestampMs":1757030400000,"label":"Doorbell"}"#)
                .expect("parse");
        assert_eq!(request.note, None);

        let draft: BookmarkDraft = request.into();
        assert_eq!(draft.label, "Doorbell");
    }

    #[test]
    fn an_absent_note_and_a_null_note_mean_different_things() {
        let untouched: UpdateBookmarkRequest =
            serde_json::from_str(r#"{"label":"Renamed"}"#).expect("parse");
        assert_eq!(untouched.note, None);

        let cleared: UpdateBookmarkRequest =
            serde_json::from_str(r#"{"note":null}"#).expect("parse");
        assert_eq!(cleared.note, Some(None));
    }

    #[test]
    fn bookmarks_serialise_with_camel_case_keys() {
        let json = serde_json::to_value(BookmarkDto::from(Bookmark {
            id: 7,
            timestamp_ms: 1_757_030_400_000,
            label: "Doorbell".to_string(),
            note: None,
            created_at_ms: 1_757_030_500_000,
        }))
        .expect("serialise");

        assert_eq!(json["timestampMs"], 1_757_030_400_000_i64);
        assert_eq!(json["createdAtMs"], 1_757_030_500_000_i64);
        assert!(json["note"].is_null());
    }
}
