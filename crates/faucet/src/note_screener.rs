//! Implements a custom note screener that keeps the faucet's tracked output notes and the notes
//! that fund the operator. It discards all other notes.
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::rpc::domain::note::CommittedNote;
use miden_client::store::{InputNoteRecord, NoteFilter, SettingScope, Store, StoreError};
use miden_client::sync::{NoteUpdateAction, OnNoteReceived};
use miden_client::utils::Deserializable;
use miden_client::{ClientError, async_trait};

use crate::{DEFAULT_OPERATOR_ACCOUNT_ID_SETTING, is_note_payable_to};

/// Provides functionality for testing whether a note is relevant to the faucet.
///
/// A note is relevant if it is a tracked output note, or a public P2ID or P2IDE note payable to
/// the operator in the chain's fee asset.
#[derive(Clone)]
pub struct NoteScreener {
    /// A reference to the faucet's store, used to fetch tracked output notes, the operator account
    /// and the chain's fee parameters.
    store: Arc<dyn Store>,
}

impl NoteScreener {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    /// Reads the operator account the faucet recorded when it was initialized.
    async fn operator_account_id(&self) -> Result<AccountId, StoreError> {
        let value = self
            .store
            .get_setting(SettingScope::User, DEFAULT_OPERATOR_ACCOUNT_ID_SETTING.to_owned())
            .await?
            .ok_or_else(|| {
                StoreError::QueryError(format!(
                    "setting {DEFAULT_OPERATOR_ACCOUNT_ID_SETTING} is not set"
                ))
            })?;

        Ok(AccountId::read_from_bytes(&value)?)
    }

    /// Reads the chain's fee faucet from the latest block header the store holds.
    async fn fee_faucet_id(&self) -> Result<AccountId, StoreError> {
        let sync_height = self.store.get_sync_height().await?;
        let (block_header, _) = self
            .store
            .get_block_header_by_num(sync_height)
            .await?
            .ok_or(StoreError::BlockHeaderNotFound(sync_height))?;

        Ok(block_header.fee_parameters().fee_faucet_id())
    }
}

#[async_trait(?Send)]
impl OnNoteReceived for NoteScreener {
    /// Queries the store for the committed note to check if it's a tracked output note, and
    /// otherwise checks whether it funds the operator.
    async fn on_note_received(
        &self,
        committed_note: CommittedNote,
        public_note: Option<InputNoteRecord>,
    ) -> Result<NoteUpdateAction, ClientError> {
        let note_id = *committed_note.note_id();

        let output_note_present =
            !self.store.get_output_notes(NoteFilter::Unique(note_id)).await?.is_empty();
        if output_note_present {
            // The note is being tracked by the client so it is relevant
            return Ok(NoteUpdateAction::Commit(committed_note));
        }

        if let Some(note) = public_note {
            // Track the P2ID notes that fund the operator account
            let operator_account_id = self.operator_account_id().await?;
            let fee_faucet_id = self.fee_faucet_id().await?;
            if is_note_payable_to(note.details(), operator_account_id, fee_faucet_id) {
                return Ok(NoteUpdateAction::Insert(note));
            }
        }

        Ok(NoteUpdateAction::Discard)
    }
}
