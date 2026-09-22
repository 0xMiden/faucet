//! Implements a custom note screener that keeps the faucet's tracked output notes and discards all
//! other notes.
use std::sync::Arc;

use miden_client::rpc::domain::note::CommittedNote;
use miden_client::store::{InputNoteRecord, NoteFilter, Store};
use miden_client::sync::{NoteUpdateAction, OnNoteReceived};
use miden_client::{ClientError, async_trait};

/// Provides functionality for testing whether a note is relevant to the faucet.
///
/// A note is relevant if it is a tracked output note.
#[derive(Clone)]
pub struct NoteScreener {
    /// A reference to the faucet's store, used to fetch tracked output notes.
    store: Arc<dyn Store>,
}

impl NoteScreener {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }
}

#[async_trait(?Send)]
impl OnNoteReceived for NoteScreener {
    /// Queries the store for the committed note to check if it's a tracked output note.
    async fn on_note_received(
        &self,
        committed_note: CommittedNote,
        _public_note: Option<InputNoteRecord>,
    ) -> Result<NoteUpdateAction, ClientError> {
        let note_id = *committed_note.note_id();

        let output_note_present =
            !self.store.get_output_notes(NoteFilter::Unique(note_id)).await?.is_empty();
        if output_note_present {
            // The note is being tracked by the client so it is relevant
            return Ok(NoteUpdateAction::Commit(committed_note));
        }

        Ok(NoteUpdateAction::Discard)
    }
}
