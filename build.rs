//! The app version comes from the git tag (e.g. `v1.0.0` -> `1.0.0`):
//! 1. LIGHTSCIO_VERSION env var (set by the release workflow from the tag),
//! 2. otherwise `git describe --tags --abbrev=0`,
//! 3. otherwise the version in Cargo.toml.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=LIGHTSCIO_VERSION");
    for p in [".git/HEAD", ".git/refs/tags", "../.git/HEAD", "../.git/refs/tags"] {
        if std::path::Path::new(p).exists() {
            println!("cargo:rerun-if-changed={p}");
        }
    }
    let from_env = std::env::var("LIGHTSCIO_VERSION").ok().filter(|v| !v.trim().is_empty());
    let from_git = || {
        Command::new("git")
            .args(["describe", "--tags", "--abbrev=0"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let v = from_env
        .or_else(from_git)
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into()));
    let v = v.trim().trim_start_matches(['v', 'V']).to_string();
    println!("cargo:rustc-env=APP_VERSION={v}");
}
