//! Bakes in the game server's key and the version, from the environment
//! (POTLUCK_SERVER_KEY, POTLUCK_VERSION) or, for builds that can't pass the
//! environment through (snaps), from `server.key` and `VERSION` files at the
//! top of the repository. Neither file is kept in git.
//!
//! Also builds in the block textures baked by tools/blocks.py, if any, from
//! assets/blocks.

fn main() {
    baked_blocks();
    for (var, file) in [("POTLUCK_SERVER_KEY", "../../server.key"), ("POTLUCK_VERSION", "../../VERSION")] {
        println!("cargo:rerun-if-env-changed={var}");
        // a missing file counts as changed on every build, so watch the
        // folder it would appear in instead
        let watch = if std::path::Path::new(file).exists() { file } else { "../.." };
        println!("cargo:rerun-if-changed={watch}");
        if std::env::var(var).map(|v| !v.is_empty()).unwrap_or(false) {
            continue;
        }
        if let Ok(v) = std::fs::read_to_string(file) {
            let v = v.trim();
            if !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || "._+-~".contains(c)) {
                println!("cargo:rustc-env={var}={v}");
            }
        }
    }
}

/// Writes `baked_blocks.rs`, a list of every PNG in assets/blocks for
/// textures.rs to build in.
fn baked_blocks() {
    let dir = std::path::Path::new("../../assets/blocks");
    println!("cargo:rerun-if-changed={}", if dir.exists() { "../../assets/blocks" } else { "../../assets" });
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "png")).collect())
        .unwrap_or_default();
    files.sort();
    let mut out = String::from("pub const BAKED: &[(&str, &[u8])] = &[\n");
    for p in files {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let abs = std::fs::canonicalize(&p).unwrap();
        out += &format!("    ({name:?}, include_bytes!({:?})),\n", abs.to_string_lossy());
    }
    out += "];\n";
    let path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("baked_blocks.rs");
    std::fs::write(path, out).unwrap();
}
