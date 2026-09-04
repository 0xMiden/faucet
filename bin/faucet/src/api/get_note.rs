use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use base64::Engine;
use base64::engine::general_purpose;
use http::StatusCode;
use miden_client::note::{NoteDetails, NoteFile, NoteId, NoteSyncHint};
use miden_client::utils::Serializable;
use miden_faucet_lib::CachedP2idNote;
use serde::Deserialize;
use tracing::instrument;

use crate::COMPONENT;
use crate::api::ApiServer;

// ENDPOINT
// ================================================================================================

#[instrument(
    parent = None, target = COMPONENT, name = "server.get_note", skip_all, err,
    fields(
        note_id = %request.note_id,
    )
)]
pub async fn get_note(
    State(server): State<ApiServer>,
    Query(request): Query<RawNoteRequest>,
) -> Result<impl IntoResponse, NoteRequestError> {
    let request = request.validate()?;

    // The P2ID note is minted by the network from the faucet's MINT note, so it never reaches the
    // client store. It is served from the cache the faucet populates at mint time instead.
    let CachedP2idNote { note, after_block_num } = {
        let cache = server.p2id_notes.read().expect("p2id note cache is poisoned");
        cache.get(&request.note_id.to_hex()).cloned()
    }
    .ok_or(NoteRequestError::NoteNotFound)?;

    let tag = note.metadata().tag();
    let (assets, _metadata, recipient, _attachments) = note.into_parts();
    let note_file = NoteFile::ExpectedNote {
        details: NoteDetails::new(assets, recipient),
        sync_hint: NoteSyncHint::new(after_block_num, tag),
    };

    let encoded_note = general_purpose::STANDARD.encode(note_file.to_bytes());
    let note_json = serde_json::json!({
        "note_id": request.note_id.to_string(),
        "data_base64": encoded_note,
    });

    Ok(Json(note_json))
}

// REQUEST VALIDATION
// ================================================================================================

/// Used to receive the initial `get_note` request from the user.
#[derive(Deserialize)]
pub struct RawNoteRequest {
    pub note_id: String,
}

impl RawNoteRequest {
    pub fn validate(self) -> Result<NoteRequest, NoteRequestError> {
        let note_id =
            NoteId::try_from_hex(&self.note_id).map_err(|_| NoteRequestError::InvalidNoteId)?;
        Ok(NoteRequest { note_id })
    }
}

/// Validated and parsed `RawNoteRequest`.
pub struct NoteRequest {
    pub note_id: NoteId,
}

#[derive(Debug, thiserror::Error)]
pub enum NoteRequestError {
    #[error("note ID failed to parse")]
    InvalidNoteId,
    #[error("note not found")]
    NoteNotFound,
}

impl NoteRequestError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidNoteId => StatusCode::BAD_REQUEST,
            // The note id parsed, so the request itself is well formed; the note is simply not in
            // the cache, either because it was never minted here or because it has been pruned.
            Self::NoteNotFound => StatusCode::NOT_FOUND,
        }
    }

    /// Take care to not expose internal errors here.
    fn user_facing_error(&self) -> String {
        match self {
            Self::InvalidNoteId => "Invalid Note ID".to_owned(),
            Self::NoteNotFound => "Note not found".to_owned(),
        }
    }
}

impl IntoResponse for NoteRequestError {
    fn into_response(self) -> axum::response::Response {
        (self.status_code(), self.user_facing_error()).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_malformed_note_id_is_a_client_error() {
        assert_eq!(NoteRequestError::InvalidNoteId.status_code(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn a_note_that_is_not_cached_is_not_found() {
        assert_eq!(NoteRequestError::NoteNotFound.status_code(), StatusCode::NOT_FOUND);
    }
}
