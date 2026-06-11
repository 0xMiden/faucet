use axum::extract::{Query, State};
use axum::response::IntoResponse;
use http::StatusCode;
use miden_client::note::{Note, NoteDetails};
use miden_client::note_transport::NoteTransportError;
use miden_client::store::NoteFilter;
use miden_client::utils::Serializable;
use tracing::{Instrument, info_span, instrument};

use crate::COMPONENT;
use crate::api::ApiServer;
use crate::api::get_note::RawNoteRequest;

// ENDPOINT
// ================================================================================================

#[instrument(
    parent = None, target = COMPONENT, name = "server.send_note", skip_all, err,
    fields(
        note_id = %request.note_id,
    )
)]
pub async fn send_note(
    State(server): State<ApiServer>,
    Query(request): Query<RawNoteRequest>,
) -> Result<(), SendNoteError> {
    let note_transport_client = server
        .note_transport_client
        .ok_or(SendNoteError::NoteTransportError(NoteTransportError::Disabled))?;

    let request = request.validate().map_err(|_| SendNoteError::InvalidNoteId)?;
    let note_record = server
        .store
        .get_output_notes(NoteFilter::Unique(request.note_id))
        .instrument(info_span!(target: COMPONENT, "store.get_output_notes"))
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to read note from store");
            SendNoteError::NoteNotFound
        })?
        .pop()
        .ok_or(SendNoteError::NoteNotFound)?;

    let note = Note::try_from(note_record).unwrap();
    let header = *note.header();
    let details: NoteDetails = note.into();

    note_transport_client.send_note(header, details.to_bytes()).await?;
    Ok(())
}

// ERRORS
// ================================================================================================

#[derive(Debug, thiserror::Error)]
pub enum SendNoteError {
    #[error("note transport layer error: {0}")]
    NoteTransportError(#[from] NoteTransportError),
    #[error("invalid note ID")]
    InvalidNoteId,
    #[error("note not found")]
    NoteNotFound,
}

impl SendNoteError {
    /// Take care to not expose internal errors here.
    fn user_facing_error(&self) -> String {
        match self {
            Self::NoteTransportError(_) => {
                "Failed to send note through note transport layer".to_owned()
            },
            Self::InvalidNoteId => "Invalid Note ID".to_owned(),
            Self::NoteNotFound => "Note not found".to_owned(),
        }
    }
}

impl IntoResponse for SendNoteError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::BAD_REQUEST, self.user_facing_error()).into_response()
    }
}
