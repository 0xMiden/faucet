use std::path::Path;

use anyhow::{Context, bail};
use miden_client::Deserializable;
use miden_client::utils::Serializable;
use miden_client::vm::Package;
use miden_standards::account::components::basic_fungible_faucet_library;

/// Compiles a Miden project, optionally linking additional libraries.
pub fn compile_dir_with_libs(
    dir: &Path,
    release: bool,
    link_libraries: &[&Path],
) -> anyhow::Result<Package> {
    let profile = if release { "--release" } else { "--debug" };
    let manifest_path = dir.join("Cargo.toml");
    let manifest_arg = manifest_path.to_string_lossy();

    let mut args = vec![
        "cargo".to_string(),
        "miden".to_string(),
        "build".to_string(),
        profile.to_string(),
        "--manifest-path".to_string(),
        manifest_arg.to_string(),
    ];
    for lib_path in link_libraries {
        args.push("--link-library".to_string());
        args.push(lib_path.to_string_lossy().to_string());
    }

    let output = run(args.into_iter(), OutputType::Masm)
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

/// Writes the official `BasicFungibleFaucet` account component as a `.masl` library
/// to the given directory, returning the path to the written file.
///
/// The Miden compiler (`cargo miden`) accepts `.masl` libraries as link libraries
/// via `--link-library`.
pub fn write_faucet_component_masl(dir: &Path) -> anyhow::Result<std::path::PathBuf> {
    let lib = basic_fungible_faucet_library();

    std::fs::create_dir_all(dir)?;
    let masl_path = dir.join("basic_fungible_faucet.masl");
    std::fs::write(&masl_path, lib.to_bytes()).context("Failed to write faucet .masl")?;

    Ok(masl_path)
}
