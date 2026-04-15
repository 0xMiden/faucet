//! Faucet account component.
//!
//! This component exposes the `mint_and_send` procedure for the faucet account. It uses kernel
//! procedures (`faucet::create_fungible_asset`, `faucet::mint`, `output_note::create`,
//! `output_note::add_asset`) which can only be called from within an account component context —
//! not directly from a transaction script.

#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, NoteType, Recipient, Tag, component, faucet, output_note};

#[component]
struct FaucetAccount;

#[component]
impl FaucetAccount {
    /// Mints a fungible asset and sends it to `recipient` by creating an output note.
    ///
    /// # Arguments
    /// - `amount`: the number of tokens to mint (in base units)
    /// - `tag`: the note tag included in the output note metadata
    /// - `note_type`: the visibility of the note (public or private)
    /// - `recipient`: the note recipient digest
    pub fn mint_and_send(
        &mut self,
        amount: Felt,
        tag: Tag,
        note_type: NoteType,
        recipient: Recipient,
    ) {
        let asset = faucet::create_fungible_asset(amount);
        faucet::mint(asset);
        let note_idx = output_note::create(tag, note_type, recipient);
        output_note::add_asset(asset, note_idx);
    }
}
