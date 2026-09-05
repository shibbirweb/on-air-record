//! A named moment on the timeline.
//!
//! Bookmarks address a point in time rather than a segment, because the interesting moment is often
//! marked while the segment covering it is still open, and because the timestamp remains meaningful
//! after the surrounding audio has been pruned.

use crate::error::{AppError, AppResult};

/// Longest label accepted. Enough for a sentence, short enough to draw beside a marker.
pub const LABEL_MAX_CHARS: usize = 120;

/// Longest note accepted, for the detail that does not fit in a label.
pub const NOTE_MAX_CHARS: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub id: i64,
    pub timestamp_ms: i64,
    pub label: String,
    pub note: Option<String>,
    pub created_at_ms: i64,
}

/// Values needed to create a bookmark, before the database assigns an id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkDraft {
    pub timestamp_ms: i64,
    pub label: String,
    pub note: Option<String>,
}

impl BookmarkDraft {
    /// Trim the text and reject anything that could not be shown usefully.
    ///
    /// Over long text is rejected rather than truncated. Silently discarding the end of what somebody
    /// typed is worse than telling them it was too long, which is the opposite of how the settings are
    /// handled: a number out of range has an obvious nearest sensible value, a sentence does not.
    pub fn sanitised(self) -> AppResult<Self> {
        let label = self.label.trim().to_string();
        if label.is_empty() {
            return Err(AppError::bad_request("a bookmark needs a label"));
        }
        if label.chars().count() > LABEL_MAX_CHARS {
            return Err(AppError::bad_request(format!(
                "the label is too long, keep it under {LABEL_MAX_CHARS} characters"
            )));
        }

        let note = self
            .note
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if let Some(note) = &note {
            if note.chars().count() > NOTE_MAX_CHARS {
                return Err(AppError::bad_request(format!(
                    "the note is too long, keep it under {NOTE_MAX_CHARS} characters"
                )));
            }
        }

        Ok(Self {
            timestamp_ms: self.timestamp_ms,
            label,
            note,
        })
    }
}

/// A partial update to an existing bookmark.
#[derive(Debug, Clone, Default)]
pub struct BookmarkPatch {
    pub timestamp_ms: Option<i64>,
    pub label: Option<String>,
    /// `Some(None)` clears the note.
    pub note: Option<Option<String>>,
}

impl BookmarkPatch {
    pub fn is_empty(&self) -> bool {
        self.timestamp_ms.is_none() && self.label.is_none() && self.note.is_none()
    }

    /// Apply to an existing bookmark and validate the result.
    pub fn apply_to(&self, existing: &Bookmark) -> AppResult<BookmarkDraft> {
        BookmarkDraft {
            timestamp_ms: self.timestamp_ms.unwrap_or(existing.timestamp_ms),
            label: self.label.clone().unwrap_or_else(|| existing.label.clone()),
            note: match &self.note {
                Some(note) => note.clone(),
                None => existing.note.clone(),
            },
        }
        .sanitised()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(label: &str) -> BookmarkDraft {
        BookmarkDraft {
            timestamp_ms: 1_757_030_400_000,
            label: label.to_string(),
            note: None,
        }
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let clean = draft("  Interview starts  ").sanitised().expect("valid");
        assert_eq!(clean.label, "Interview starts");
    }

    #[test]
    fn rejects_a_label_that_is_only_whitespace() {
        assert!(draft("   ").sanitised().is_err());
        assert!(draft("").sanitised().is_err());
    }

    #[test]
    fn rejects_a_label_that_is_too_long() {
        let long = "a".repeat(LABEL_MAX_CHARS + 1);
        assert!(draft(&long).sanitised().is_err());
        // The boundary itself is fine.
        assert!(draft(&"a".repeat(LABEL_MAX_CHARS)).sanitised().is_ok());
    }

    #[test]
    fn counts_characters_rather_than_bytes() {
        // A label of accented characters is not secretly half as long as it looks.
        let accented = "e\u{0301}".repeat(LABEL_MAX_CHARS / 2);
        assert!(draft(&accented).sanitised().is_ok());
    }

    #[test]
    fn an_empty_note_becomes_absent() {
        let cleaned = BookmarkDraft {
            note: Some("   ".to_string()),
            ..draft("Marker")
        }
        .sanitised()
        .expect("valid");
        assert_eq!(cleaned.note, None);
    }

    #[test]
    fn a_patch_leaves_untouched_fields_alone() {
        let existing = Bookmark {
            id: 1,
            timestamp_ms: 1_000,
            label: "Original".to_string(),
            note: Some("why".to_string()),
            created_at_ms: 500,
        };

        let renamed = BookmarkPatch {
            label: Some("Renamed".to_string()),
            ..BookmarkPatch::default()
        }
        .apply_to(&existing)
        .expect("valid");

        assert_eq!(renamed.label, "Renamed");
        assert_eq!(renamed.timestamp_ms, 1_000);
        assert_eq!(renamed.note, Some("why".to_string()));
    }

    #[test]
    fn a_patch_can_clear_the_note() {
        let existing = Bookmark {
            id: 1,
            timestamp_ms: 1_000,
            label: "Original".to_string(),
            note: Some("why".to_string()),
            created_at_ms: 500,
        };

        let cleared = BookmarkPatch {
            note: Some(None),
            ..BookmarkPatch::default()
        }
        .apply_to(&existing)
        .expect("valid");

        assert_eq!(cleared.note, None);
    }

    #[test]
    fn a_patch_that_empties_the_label_is_rejected() {
        let existing = Bookmark {
            id: 1,
            timestamp_ms: 1_000,
            label: "Original".to_string(),
            note: None,
            created_at_ms: 500,
        };

        assert!(BookmarkPatch {
            label: Some("  ".to_string()),
            ..BookmarkPatch::default()
        }
        .apply_to(&existing)
        .is_err());
    }
}
