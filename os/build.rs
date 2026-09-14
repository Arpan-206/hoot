//! Build script: places `memory.x` for the linker and bakes optional
//! defaults from `secrets.toml` into the firmware as environment variables.

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("memory.x"))
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=secrets.toml");

    // secrets.toml is git-ignored. Each `key = "value"` line becomes
    // HOOT_<KEY> for `option_env!`. Missing file: no defaults baked in.
    if let Ok(text) = fs::read_to_string("secrets.toml") {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim().to_ascii_uppercase();
            let value = value.trim().trim_matches('"');
            if key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                println!("cargo:rustc-env=HOOT_{key}={value}");
            }
        }
    }
}
