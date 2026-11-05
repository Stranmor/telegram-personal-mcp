// The bridge names the exact TL stack it was built with in every schema
// mismatch event, so an "unexpected constructor id" points at the library that
// cannot decode the server's constructor instead of looking like a Telegram
// outage. The versions come from Cargo.lock, so a dependency bump updates the
// report without a second edit.

/// Package versions that make up the client TL schema.
const TL_STACK_CRATES: [&str; 2] = ["ferogram-tl-types", "ferogram"];

fn main() {
    println!("cargo:rerun-if-changed=Cargo.lock");
    let lock = std::fs::read_to_string("Cargo.lock").unwrap_or_default();
    let packages = parse_lock(&lock);
    for name in TL_STACK_CRATES {
        let version = packages
            .iter()
            .find(|(package, _)| package == name)
            .map(|(_, version)| version.clone())
            .unwrap_or_else(|| "unknown".to_string());
        println!(
            "cargo:rustc-env={}={version}",
            version_env_name(name)
        );
    }
    if !packages.iter().any(|(package, _)| package == "ferogram-tl-types") {
        println!(
            "cargo:warning=Cargo.lock has no ferogram-tl-types entry; TL schema mismatch events will report an unknown client library"
        );
    }
}

/// Environment variable a crate version is reported through.
fn version_env_name(crate_name: &str) -> String {
    format!("TL_STACK_{}", crate_name.to_uppercase().replace('-', "_"))
}

/// Minimal Cargo.lock reader: one `(name, version)` per package.
fn parse_lock(lock: &str) -> Vec<(String, String)> {
    let mut packages = Vec::new();
    for block in lock.split("[[package]]").skip(1) {
        let mut name = None;
        let mut version = None;
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("name = \"") {
                name = Some(value.trim_end_matches('"').to_string());
            } else if let Some(value) = line.strip_prefix("version = \"") {
                version = Some(value.trim_end_matches('"').to_string());
            }
        }
        if let (Some(name), Some(version)) = (name, version) {
            packages.push((name, version));
        }
    }
    packages
}
