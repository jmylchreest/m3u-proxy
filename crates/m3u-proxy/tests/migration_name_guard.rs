use std::collections::{HashMap, HashSet};
use std::path::Path;

use sea_orm_migration::MigratorTrait;

/// Guard test to ensure all migrations have:
/// 1. Unique names (no collisions like the prior "mod")
/// 2. A well-formed timestamped name: mYYYYMMDD_HHMMSS_description
///    - Must start with 'm'
///    - Date segment: 8 digits (YYYYMMDD)
///    - Time segment: 6 digits (HHMMSS)
///    - Description: lowercase alphanumeric + underscores, at least 1 char
/// 3. Any folder-based migration (directory under migrations/) has a name that EXACTLY
///    matches the folder name (prevents silent divergence between folder and reported name)
///
/// If this test fails:
/// - Implement `MigrationName` (or macro) so the folder name and reported name align
/// - Fix naming format
///
/// This test intentionally fails fast with a detailed diagnostic message so
/// mistakes are caught during development / CI instead of at runtime.
#[test]
fn migration_names_are_unique_and_well_formed() {
    // Bring the project's migrator into scope.
    // The crate name `m3u-proxy` (with hyphen) becomes `m3u_proxy` as a Rust identifier.
    use m3u_proxy::database::migrations::Migrator;

    let migrations = Migrator::migrations();

    let mut seen: HashSet<String> = HashSet::new();
    let mut duplicates: HashMap<String, usize> = HashMap::new();
    let mut invalid: Vec<String> = Vec::new();

    for mig in migrations.iter() {
        let name = mig.name().to_string();

        if !is_valid_migration_name(&name) {
            invalid.push(name.clone());
        }

        if !seen.insert(name.clone()) {
            *duplicates.entry(name).or_insert(1) += 1;
        }
    }

    // Collect folder names in the migrations directory that look like migrations
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let migrations_dir = Path::new(manifest_dir)
        .join("src")
        .join("database")
        .join("migrations");
    let mut folder_mismatches: Vec<String> = Vec::new();

    if let Ok(read_dir) = std::fs::read_dir(&migrations_dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(folder_name) = path.file_name().and_then(|s| s.to_str()) {
                    // Consider only migration-style folders starting with 'm' + digits + '_'
                    if folder_name.starts_with('m')
                        && folder_name.len() > 16
                        && folder_name.chars().nth(15) == Some('_')
                        && !seen.contains(folder_name)
                    {
                        folder_mismatches.push(folder_name.to_string());
                    }
                }
            }
        }
    }

    if !duplicates.is_empty() || !invalid.is_empty() || !folder_mismatches.is_empty() {
        let mut msg = String::from("Migration naming guard failed:\n");

        if !duplicates.is_empty() {
            msg.push_str("  Duplicate names detected:\n");
            for (name, count) in duplicates {
                msg.push_str(&format!("    * {} (occurrences: {})\n", name, count + 1));
            }
        }

        if !invalid.is_empty() {
            msg.push_str("  Invalid naming format (expected mYYYYMMDD_HHMMSS_description):\n");
            for name in invalid {
                msg.push_str(&format!("    * {}\n", name));
            }
        }

        if !folder_mismatches.is_empty() {
            msg.push_str("  Folder/name mismatches (directory names not reported by Migrator):\n");
            for f in folder_mismatches {
                msg.push_str(&format!("    * {}\n", f));
            }
        }

        panic!("{msg}");
    }
}

/// Validate migration name format:
/// m + 8 digits + '_' + 6 digits + '_' + description
/// Description: [a-z0-9_]+
fn is_valid_migration_name(name: &str) -> bool {
    // Quick length + structural checks
    if !name.starts_with('m') {
        return false;
    }
    let parts: Vec<&str> = name[1..].split('_').collect();
    if parts.len() < 3 {
        return false;
    }

    let date = parts[0];
    let time = parts[1];
    let desc = &parts[2..].join("_"); // Allow extra underscores inside description

    if date.len() != 8 || !date.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if time.len() != 6 || !time.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if desc.is_empty()
        || !desc
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return false;
    }

    true
}
