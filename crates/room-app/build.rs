//! Embeds the checkout's `assets/` (the 3D models, about 4 MB) into the binary, so an
//! app installed with `cargo install` works after its build folder is gone (PLAN.md M21).
//! Writes `$OUT_DIR/embedded_assets.rs`: a list of (path relative to `assets/`, bytes).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let root = root.canonicalize().expect("assets/ next to the workspace");
    println!("cargo:rerun-if-changed={}", root.display());

    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();

    let mut out = String::from("&[\n");
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
        let rel = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        writeln!(out, "    ({rel:?}, include_bytes!({:?}) as &[u8]),", file.display().to_string()).unwrap();
    }
    out.push_str("]\n");

    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("embedded_assets.rs");
    std::fs::write(dest, out).unwrap();
}

/// Only what the app loads: glTF models, textures and shaders. READMEs and licences stay in the repo.
fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            println!("cargo:rerun-if-changed={}", path.display());
            walk(&path, files);
        } else if path.extension().is_some_and(|e| e == "glb" || e == "gltf" || e == "bin" || e == "png" || e == "ktx2" || e == "wesl") {
            files.push(path);
        }
    }
}
