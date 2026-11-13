//! Drift guard: every serialized struct on the wire must carry
//! `#[serde(rename_all = "camelCase")]`, so a new contract cannot accidentally
//! ship snake_case keys the camelCase frontend won't read. Enums are exempt —
//! their variant *values* stay snake/lowercase, which the frontend's
//! string-literal types depend on — and unit/marker structs have no fields.

use std::fs;
use std::path::Path;

#[test]
fn every_wire_struct_is_camel_case() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    for entry in fs::read_dir(&dir)
        .unwrap()
        .chain(fs::read_dir(dir.join("plugin")).unwrap())
    {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let file = path.file_name().unwrap().to_str().unwrap().to_string();
        scan(&fs::read_to_string(&path).unwrap(), &file, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "these serialized structs are missing #[serde(rename_all = \"camelCase\")] — \
         add it, or if a struct is a deliberate exception allowlist it with a reason \
         (see docs/contributing.md):\n{}",
        violations.join("\n")
    );
}

fn scan(src: &str, file: &str, out: &mut Vec<String>) {
    let lines: Vec<&str> = src.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !(trimmed.starts_with("pub struct ") && line.trim_end().ends_with('{')) {
            continue;
        }
        let name = trimmed["pub struct ".len()..]
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .unwrap_or("");
        let (mut has_serde, mut has_rename) = (false, false);
        let mut j = i;
        while j > 0 {
            j -= 1;
            let attr = lines[j].trim_start();
            if !(attr.starts_with("#[") || attr.starts_with("//")) {
                break;
            }
            if attr.contains("derive(") && attr.contains("erialize") {
                has_serde = true;
            }
            if attr.contains("rename_all") {
                has_rename = true;
            }
        }
        if has_serde && !has_rename {
            out.push(format!("  {file}: {name}"));
        }
    }
}
