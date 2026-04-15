//! Faucet mint transaction script.
//!
//! This script mints fungible assets and distributes them to recipients by creating output
//! notes.
//!
//! # Script argument
//!
//! The script receives a single `Word` argument (`arg`) which is the RPO hash commitment
//! of the note data stored in the advice map.
//!
//! # Advice map
//!
//! The advice map must contain an entry keyed by `arg` with the following layout:
//!
//! ```text
//! [RECIPIENT_1, note_type_1, tag_1, amount_1, RECIPIENT_2, note_type_2, tag_2, amount_2, ..., padding]
//! ```
//!
//! Where for each note:
//! - `RECIPIENT` (4 felts): the note recipient digest
//! - `note_type` (1 felt): the note type (1 = public, 2 = private)
//! - `tag` (1 felt): the note tag
//! - `amount` (1 felt): the amount of tokens to mint
//!
//! The data must be padded with zeros to the next multiple of 4 felts (word-aligned).
//!
//! # Account component
//!
//! The script calls `account.mint_and_send()` from the faucet account component, which internally
//! creates a fungible asset, mints it via the kernel, and creates an output note with the asset.

#![no_std]
#![feature(alloc_error_handler)]

use miden::intrinsics::advice::adv_push_mapvaln;
use miden::tx::update_expiration_block_delta;
use miden::{Felt, Recipient, Word, pipe_words_to_memory, tx_script};

use crate::bindings::Account;

/// Number of felts per note in the advice map data.
///
/// Layout: `[RECIPIENT(4), note_type(1), tag(1), amount(1)]`
const NOTE_ARGS_SIZE: usize = 7;

/// Transaction expiration delta in blocks.
const EXPIRATION_DELTA: u32 = 10;

#[tx_script]
fn run(arg: Word, account: &mut Account) {
    update_expiration_block_delta(Felt::from_u32(EXPIRATION_DELTA));

    // Push note data from the advice map onto the advice stack using the commitment as key.
    let num_felts = adv_push_mapvaln(arg);
    let num_felts_u64 = num_felts.as_canonical_u64();

    // Pop the data from the advice stack into memory (requires word-aligned length).
    let num_words = Felt::new((num_felts_u64 + 3) / 4);
    let (_hash, input) = pipe_words_to_memory(num_words);

    let num_notes = num_felts_u64 as usize / NOTE_ARGS_SIZE;

    for idx in 0..num_notes {
        let start = idx * NOTE_ARGS_SIZE;
        let recipient = Recipient::from(Word::from([
            input[start],
            input[start + 1],
            input[start + 2],
            input[start + 3],
        ]));
        let note_type = input[start + 4].into();
        let tag = input[start + 5].into();
        let amount = input[start + 6];

        account.mint_and_send(amount, tag, note_type, recipient);
    }
}
