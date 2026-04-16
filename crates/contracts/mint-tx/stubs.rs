/// Linker stub for the `mint_and_send` procedure from `BasicFungibleFaucet`.
///
/// This is an unreachable stub that satisfies the Wasm linker. The actual procedure
/// is resolved by the Miden compiler against the official component library.
#[unsafe(export_name = "miden::standards::faucets::basic_fungible::mint_and_send")]
pub extern "C" fn mint_and_send_stub(
    _amount: f32,
    _tag: f32,
    _note_type: f32,
    _recipient_0: f32,
    _recipient_1: f32,
    _recipient_2: f32,
    _recipient_3: f32,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
