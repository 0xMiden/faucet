use axum::extract::{Query, State};
use axum::response::IntoResponse;
use http::StatusCode;
use miden_client::note::NoteDetails;
use miden_client::note_transport::NoteTransportError;
use miden_client::utils::Serializable;
use miden_faucet_lib::CachedP2idNote;
use tracing::{info, instrument};

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

    // The P2ID note is minted by the network from the faucet's MINT note, so it never reaches the
    // client store. It is served from the cache the faucet populates at mint time instead.
    let CachedP2idNote { note, after_block_num } = {
        let cache = server.p2id_notes.read().expect("p2id note cache is poisoned");
        cache.get(&request.note_id.to_hex()).cloned()
    }
    .ok_or(SendNoteError::NoteNotFound)?;

    let header = *note.header();
    let details: NoteDetails = note.into();

    note_transport_client
        .send_note_with_block_hint(header, details.to_bytes(), after_block_num)
        .await?;
    info!(
        target: COMPONENT,
        {
            note.id = %request.note_id.to_hex(),
            after_block_num = %after_block_num
        },
        "Relayed private note through the note transport layer",
    );

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
    fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidNoteId => StatusCode::BAD_REQUEST,
            Self::NoteNotFound => StatusCode::NOT_FOUND,
            // The note transport layer is a server-side dependency, so none of its failures are
            // caused by the request. `Disabled` is a deployment that was never configured with a
            // transport URL, which no client can correct by retrying differently; every other
            // variant is the upstream service being unreachable or answering unusably.
            Self::NoteTransportError(NoteTransportError::Disabled) => StatusCode::NOT_IMPLEMENTED,
            Self::NoteTransportError(_) => StatusCode::BAD_GATEWAY,
        }
    }

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
        (self.status_code(), self.user_facing_error()).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_malformed_note_id_is_a_client_error() {
        assert_eq!(SendNoteError::InvalidNoteId.status_code(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn a_note_that_is_not_cached_is_not_found() {
        assert_eq!(SendNoteError::NoteNotFound.status_code(), StatusCode::NOT_FOUND);
    }

    /// A faucet started without a transport URL cannot serve this endpoint at all. That is a
    /// deployment decision, not something the caller got wrong.
    #[test]
    fn a_disabled_transport_layer_is_not_implemented() {
        assert_eq!(
            SendNoteError::NoteTransportError(NoteTransportError::Disabled).status_code(),
            StatusCode::NOT_IMPLEMENTED,
        );
    }

    /// An unreachable or misbehaving transport service is an upstream failure, so it has to be
    /// reported as one: a 4xx would tell operators and clients that the caller was at fault.
    #[test]
    fn an_upstream_transport_failure_is_a_gateway_error() {
        for error in [
            NoteTransportError::Network("connection reset".to_string()),
            NoteTransportError::PaginationDidNotTerminate(64),
        ] {
            assert_eq!(
                SendNoteError::NoteTransportError(error).status_code(),
                StatusCode::BAD_GATEWAY,
            );
        }
    }
}
