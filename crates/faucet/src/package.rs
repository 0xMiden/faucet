use std::path::Path;

use anyhow::{Context, bail};
use cargo_miden::{OutputType, run};
use miden_client::Deserializable;
use miden_client::vm::Package;

/// Compiles a Miden project in the specified directory
///
/// # Arguments
/// * `dir` - Path to the directory containing the Cargo.toml
/// * `release` - Whether to build in release mode
///
/// # Returns
/// The compiled `Package`
///
/// # Errors
/// Returns an error if compilation fails or if the output is not in the expected format
pub fn compile_dir(dir: &Path, release: bool) -> anyhow::Result<Package> {
    let profile = if release { "--release" } else { "--debug" };
    let manifest_path = dir.join("Cargo.toml");
    let manifest_arg = manifest_path.to_string_lossy();

    let args = vec!["cargo", "miden", "build", profile, "--manifest-path", &manifest_arg];

    let output = run(args.into_iter().map(String::from), OutputType::Masm)
        .context("Failed to compile project")?
        .context("Cargo miden build returned None")?;

    let artifact_path = match output {
        cargo_miden::CommandOutput::BuildCommandOutput { output } => match output {
            cargo_miden::BuildOutput::Masm { artifact_path } => artifact_path,
            other @ cargo_miden::BuildOutput::Wasm { .. } => {
                bail!("Expected Masm output, got {other:?}")
            },
        },
        other @ cargo_miden::CommandOutput::NewCommandOutput { .. } => {
            bail!("Expected BuildCommandOutput, got {other:?}")
        },
    };

    let package_bytes = std::fs::read(&artifact_path)
        .context(format!("Failed to read compiled package from {}", artifact_path.display()))?;

    Package::read_from_bytes(&package_bytes).context("Failed to deserialize package from bytes")
}
