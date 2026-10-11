//! Drift guard: a list on the wire is always sent, even when empty. The
//! generated TypeScript types declare every `Vec` as a present array, so one
//! omitted when empty reaches the frontend as `undefined` and throws the first
//! time it is iterated.

use std::fs;
use std::path::Path;

#[test]
fn no_wire_list_is_omitted_when_empty() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let file = path.file_name().unwrap().to_str().unwrap().to_string();
        for (number, line) in fs::read_to_string(&path).unwrap().lines().enumerate() {
            if line.contains("skip_serializing_if") && line.contains("Vec::is_empty") {
                violations.push(format!("  {file}:{}", number + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "these lists are skipped when empty — send them as `[]` instead:\n{}",
        violations.join("\n")
    );
}
