use std::path::{Path, PathBuf};
use std::process::Command;

const CONTRACTS: &[&str] = &["mint-tx"];

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let contracts_dir = manifest_dir.join("../contracts");

    for contract in CONTRACTS {
        let dir = contracts_dir.join(contract);
        // println!("cargo:rerun-if-changed={}", dir.display());
        compile(&dir);
    }
}

fn compile(dir: &Path) {
    let manifest_path = dir.join("Cargo.toml");
    let status = Command::new("miden")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn `miden build` for {}: {e}", dir.display()));

    // assert!(status.success(), "`miden build` for {} exited with {status}", dir.display());
}
