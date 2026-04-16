use std::env;
use std::process::Command;

/// Compiles the `mint_and_send` linker stub into a static library that the Wasm linker
/// can use to satisfy the import. The actual procedure is resolved by the Miden compiler
/// at the MAST level.
fn main() {
    println!("cargo::rerun-if-changed=stubs.rs");

    let out_dir = env::var("OUT_DIR").unwrap();
    let target = env::var("TARGET").unwrap();

    // Only build stubs for Wasm targets
    if !target.starts_with("wasm") {
        return;
    }

    let status = Command::new("rustc")
        .arg("--crate-name")
        .arg("mint_tx_stubs")
        .arg("--edition=2024")
        .arg("--crate-type=rlib")
        .arg("--target")
        .arg(&target)
        .arg("-C")
        .arg("opt-level=1")
        .arg("-C")
        .arg("debuginfo=0")
        .arg("stubs.rs")
        .arg("-o")
        .arg(format!("{out_dir}/libmint_tx_stubs.a"))
        .status()
        .expect("failed to compile stubs");

    assert!(status.success(), "stub compilation failed");

    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=mint_tx_stubs");
}
