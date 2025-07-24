use std::env;
use std::process::Command;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    
    // Generate SBOM during build
    let output = Command::new("cargo")
        .args(&["sbom", "--output-format", "spdx_json_2_3"])
        .output()
        .expect("Failed to generate SBOM");
    
    if output.status.success() {
        // Write SBOM to a file that can be included in the binary
        std::fs::write(
            format!("{}/sbom.json", out_dir),
            output.stdout
        ).expect("Failed to write SBOM file");
    } else {
        // Fallback if cargo sbom is not available
        eprintln!("Warning: cargo sbom not available, dependency versions will not be shown");
        std::fs::write(
            format!("{}/sbom.json", out_dir),
            "{}"
        ).expect("Failed to write empty SBOM file");
    }

    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=Cargo.toml");
}