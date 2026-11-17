use std::path::Path;
use std::process::Command;
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-changed=frontend");

    let build_dir = env::var("OUT_DIR").expect("OUT_DIR should be set");
    let target_dir = Path::new(&build_dir).join("frontend");

    fs::create_dir_all(&target_dir).expect("target directory should be created");
    copy_dir_all(Path::new("frontend"), &target_dir)
        .expect("frontend directory should be copied to target directory");

    let npm_install = Command::new("npm")
        .arg("install")
        .current_dir(&target_dir)
        .status()
        .expect("npm install should succeed");

    assert!(npm_install.success(), "npm install failed");

    let npm_build = Command::new("npm")
        .arg("run")
        .arg("build")
        .current_dir(&target_dir)
        .status()
        .expect("npm run build should succeed");

    assert!(npm_build.success(), "npm run build failed");

    compress_wasm(&target_dir);
}

/// Pre-compress the SDK WASM so the server can serve the compressed bytes directly.
fn compress_wasm(target_dir: &Path) {
    let wasm_path =
        target_dir.join("node_modules/@miden-sdk/miden-sdk/dist/st/assets/miden_client_web.wasm");
    let wasm = fs::read(&wasm_path).expect("SDK wasm should be readable");

    //.Quality 9 gets close to the maximum ratio while keeping the build step fast.
    let params = brotli::enc::BrotliEncoderParams { quality: 9, ..Default::default() };
    let mut compressed = Vec::new();
    brotli::BrotliCompress(&mut wasm.as_slice(), &mut compressed, &params)
        .expect("wasm brotli compression should succeed");

    fs::write(wasm_path.with_extension("wasm.br"), compressed)
        .expect("compressed wasm should be written");
}

/// Copy all files from source directory to destination directory. Skips inner directories.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}
