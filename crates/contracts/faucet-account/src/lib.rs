//! Faucet account component stub.
//!
//! This crate exists only to generate the WIT interface for the `mint_and_send` procedure.
//! The actual account component used at runtime is the official `BasicFungibleFaucet` from
//! `miden-standards`. The `.masp` produced by compiling this crate is NOT used for linking —
//! instead, the faucet library links the official component library via `--link-library`.

#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, NoteType, Recipient, Tag, component, faucet, output_note};

#[component]
struct FaucetAccount;

#[component]
impl FaucetAccount {
    /// Mints a fungible asset and sends it to `recipient` by creating an output note.
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
