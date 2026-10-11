//! Drift guard: a field is omitted from the wire only when it is an `Option`.
//! The generated TypeScript types make an `Option` an optional key and declare
//! everything else present, so a list, string, number or flag skipped when
//! empty reaches the frontend as `undefined` — and a list throws the first time
//! it is iterated.

use std::fs;
use std::path::Path;

#[test]
fn only_an_option_is_omitted_from_the_wire() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let file = path.file_name().unwrap().to_str().unwrap().to_string();
        for (number, line) in fs::read_to_string(&path).unwrap().lines().enumerate() {
            if line.contains("skip_serializing_if") && !line.contains("Option::is_none") {
                violations.push(format!("  {file}:{}", number + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "these fields are skipped when empty — send the empty value instead:\n{}",
        violations.join("\n")
    );
}
