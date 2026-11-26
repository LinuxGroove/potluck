//! Bakes in the game server's key and the version, from the environment
//! (POTLUCK_SERVER_KEY, POTLUCK_VERSION) or, for builds that can't pass the
//! environment through (snaps), from `server.key` and `VERSION` files at the
//! top of the repository. Neither file is kept in git.

fn main() {
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
