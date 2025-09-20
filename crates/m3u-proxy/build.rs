#![allow(clippy::print_stdout)]
// Justification: build scripts must communicate with Cargo via stdout lines (cargo:rerun-if-changed=...).
// We keep output minimal and structured; no general logging via println!.

use std::{env, fs, process::Command};

fn main() {
    // If OUT_DIR is unavailable we silently abort (build proceeds without SBOM).
    let Ok(out_dir) = env::var("OUT_DIR") else {
        return;
    };

    // Attempt SBOM generation; fall back to "{}" placeholder on any failure.
    let sbom_bytes = generate_sbom().unwrap_or_else(|_| b"{}".to_vec());

    let sbom_path = format!("{out_dir}/sbom.json");
    let _ = fs::write(&sbom_path, sbom_bytes);

    // Cargo rebuild triggers
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=Cargo.toml");
}

/// Try to invoke `cargo sbom` producing SPDX JSON.
/// Returns an error if the command is missing or exits non‑zero.
fn generate_sbom() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let output = Command::new("cargo")
        .args(["sbom", "--output-format", "spdx_json_2_3"])
        .output()?;

    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err("cargo sbom returned non-zero status".into())
    }
}
