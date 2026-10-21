//! The plugin API is every contract and topic that names a `PLUGIN` permission,
//! plus the WIT package. `golden/plugin-api.json` snapshots its shape so a
//! change cannot slip past the version: an additive change bumps the minor of
//! `API_VERSION`, a breaking one the major, and either then regenerates the snapshot.
//!
//! Regenerate with `HESTIA_UPDATE_GOLDEN=1 cargo test -p proto --test plugin_api`.
//!
//! The shape is read from the source rather than reflected: `proto` carries no
//! schema derive outside the `ts` feature, and this test must run without it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

const GOLDEN: &str = "tests/golden/plugin-api.json";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
}

/// Splits on commas outside any bracket, so a field type or a variant payload stays whole.
fn split_top_level(body: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in body.chars() {
        match c {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
        if c == ',' && depth == 0 {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    parts.push(current);
    parts
        .into_iter()
        .map(|part| squash(&part))
        .filter(|part| !part.is_empty())
        .collect()
}

/// Peels leading `#[...]` attributes off a member, keeping only the serde ones.
fn split_attributes(member: &str) -> (Vec<String>, String) {
    let mut rest = member.trim();
    let mut serde = Vec::new();
    while let Some(after) = rest.strip_prefix("#[") {
        let mut depth = 1;
        let end = after
            .char_indices()
            .find(|&(_, c)| {
                match c {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    _ => {}
                }
                depth == 0
            })
            .map(|(index, _)| index)
            .expect("attribute is closed");
        let attribute = after[..end].trim();
        if attribute.starts_with("serde(") {
            serde.push(attribute.to_string());
        }
        rest = after[end + 1..].trim();
    }
    (serde, rest.to_string())
}

fn declaration_name(declaration: &str) -> &str {
    identifiers(declaration).next().expect("a declared name")
}

/// One struct, enum or alias: its serde attributes and its members by name.
fn type_definition(kind: &str, attributes: &[String], body: &str) -> Value {
    let mut members = Map::new();
    match kind {
        "alias" => {
            members.insert("=".into(), json!(squash(body)));
        }
        "tuple" => {
            members.insert("0".into(), json!(squash(body)));
        }
        _ => {
            for member in split_top_level(body) {
                let (serde, declaration) = split_attributes(&member);
                let name = declaration_name(declaration.trim_start_matches("pub "));
                members.insert(
                    name.to_string(),
                    json!(format!("{} {declaration}", serde.join(" ")).trim()),
                );
            }
        }
    }
    json!({ "kind": kind, "attributes": attributes, "members": members })
}

struct Source {
    types: BTreeMap<String, Value>,
    impls: Vec<(String, String, BTreeMap<String, String>)>,
}

/// Collects every top-level type and every `impl Contract|Topic|Job for` block under `src/`.
fn scan_sources() -> Source {
    let mut source = Source {
        types: BTreeMap::new(),
        impls: Vec::new(),
    };
    let mut files: Vec<PathBuf> = fs::read_dir(crate_dir().join("src"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("rs"))
        .collect();
    files.sort();
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let mut attributes: Vec<String> = Vec::new();
        let mut pending = String::new();
        let mut index = 0;
        while index < lines.len() {
            let line = lines[index];
            index += 1;
            if !pending.is_empty() || line.starts_with("#[") {
                pending.push_str(line);
                pending.push(' ');
                if pending.matches('[').count() == pending.matches(']').count() {
                    let (serde, _) = split_attributes(&pending);
                    attributes.extend(serde);
                    pending.clear();
                }
                continue;
            }
            if line.starts_with("///") || line.starts_with("//") {
                continue;
            }
            let item = line.strip_prefix("pub ").unwrap_or(line);
            let kind = ["struct ", "enum ", "type "]
                .into_iter()
                .find(|keyword| item.starts_with(keyword));
            let is_impl = item.starts_with("impl");
            if kind.is_none() && !is_impl {
                attributes.clear();
                continue;
            }
            let mut block = String::from(line);
            while !block.trim_end().ends_with(['{', ';', '}']) || block.trim_end().ends_with(',') {
                block.push(' ');
                block.push_str(lines[index]);
                index += 1;
            }
            let opens_body = block.trim_end().ends_with('{');
            let mut body = String::new();
            if opens_body {
                while !lines[index].starts_with('}') {
                    let inner = lines[index].trim();
                    if !inner.starts_with("//") {
                        body.push_str(inner);
                        body.push('\n');
                    }
                    index += 1;
                }
                index += 1;
            }
            if is_impl {
                record_impl(&mut source, &block, &body);
            } else {
                record_type(&mut source, kind.unwrap(), &block, &body, &attributes);
            }
            attributes.clear();
        }
    }
    source
}

fn record_type(
    source: &mut Source,
    keyword: &str,
    header: &str,
    body: &str,
    attributes: &[String],
) {
    let header = header.trim();
    let after_keyword = header.split_once(keyword).unwrap().1;
    let name = declaration_name(after_keyword).to_string();
    let definition = if keyword == "type " {
        let expression = after_keyword.split_once('=').unwrap().1;
        type_definition("alias", attributes, expression.trim_end_matches(';'))
    } else if keyword == "enum " {
        type_definition("enum", attributes, body)
    } else if header.ends_with('{') || header.ends_with("{}") {
        type_definition("struct", attributes, body)
    } else {
        let tuple = after_keyword.split_once('(').map_or("", |(_, rest)| rest);
        type_definition("tuple", attributes, tuple.trim_end_matches(");"))
    };
    let previous = source.types.insert(name.clone(), definition);
    assert!(previous.is_none(), "two proto types are named {name}");
}

fn record_impl(source: &mut Source, header: &str, body: &str) {
    let Some((trait_part, target)) = header.split_once(" for ") else {
        return;
    };
    let Some(trait_name) = ["Contract", "Topic", "Job"]
        .into_iter()
        .find(|name| trait_part.trim_end().ends_with(name))
    else {
        return;
    };
    let target = declaration_name(target).to_string();
    let mut items = BTreeMap::new();
    for line in body.lines() {
        let line = line.trim();
        let Some(rest) = line
            .strip_prefix("const ")
            .or_else(|| line.strip_prefix("type "))
        else {
            continue;
        };
        let (name, value) = rest.split_once('=').unwrap();
        let name = declaration_name(name).to_string();
        if matches!(
            name.as_str(),
            "CHANNEL" | "TOPIC" | "PLUGIN" | "Params" | "Result" | "Progress" | "Done"
        ) {
            items.insert(name, value.trim().trim_end_matches(';').to_string());
        }
    }
    source.impls.push((trait_name.to_string(), target, items));
}

/// A type and every proto type it names, transitively.
fn collect_types(source: &Source, expression: &str, found: &mut BTreeMap<String, Value>) {
    for word in identifiers(expression) {
        if let Some(definition) = source.types.get(word) {
            if found.insert(word.to_string(), definition.clone()).is_none() {
                let text = definition["members"]
                    .as_object()
                    .unwrap()
                    .values()
                    .map(|member| member.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(" ");
                collect_types(source, &text, found);
            }
        }
    }
}

fn permission(items: &BTreeMap<String, String>) -> Option<String> {
    let value = items.get("PLUGIN")?.strip_prefix("Some(Permission::")?;
    Some(value.trim_end_matches(')').to_string())
}

fn wit_files() -> BTreeMap<String, String> {
    let dir = crate_dir().join("../sdk/wit");
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("wit"))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let text = fs::read_to_string(&path).unwrap();
            (name, wit_lines(&text).join("\n"))
        })
        .collect()
}

fn current_snapshot() -> Value {
    let source = scan_sources();
    let mut channels = BTreeMap::new();
    let mut types = BTreeMap::new();
    for (trait_name, target, items) in &source.impls {
        let Some(permission) = permission(items) else {
            continue;
        };
        let mut entry = Map::new();
        entry.insert("permission".into(), json!(permission));
        let name = match trait_name.as_str() {
            "Contract" => {
                for key in ["Params", "Result"] {
                    collect_types(&source, &items[key], &mut types);
                    entry.insert(key.to_lowercase(), json!(items[key]));
                }
                if let Some((_, _, job)) = source
                    .impls
                    .iter()
                    .find(|(t, name, _)| t == "Job" && name == target)
                {
                    for key in ["Progress", "Done"] {
                        collect_types(&source, &job[key], &mut types);
                        entry.insert(key.to_lowercase(), json!(job[key]));
                    }
                }
                items["CHANNEL"].trim_matches('"').to_string()
            }
            "Topic" => {
                collect_types(&source, target, &mut types);
                entry.insert("payload".into(), json!(target));
                items["TOPIC"].trim_matches('"').to_string()
            }
            _ => continue,
        };
        channels.insert(name, Value::Object(entry));
    }
    json!({
        "apiVersion": proto::plugin::API_VERSION,
        "wit": wit_files(),
        "channels": channels,
        "types": types,
    })
}

struct Change {
    breaking: bool,
    what: String,
}

fn breaking(what: String) -> Change {
    Change {
        breaking: true,
        what,
    }
}

fn additive(what: String) -> Change {
    Change {
        breaking: false,
        what,
    }
}

fn section<'a>(snapshot: &'a Value, name: &str) -> &'a Map<String, Value> {
    snapshot[name].as_object().expect("snapshot section")
}

/// The package line carries the version, which `apiVersion` already records.
fn wit_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("package ") && !line.starts_with("//"))
        .collect()
}

/// Lines whose addition breaks an installed plugin: wasmtime checks types structurally, so a
/// new case, record field or flag changes a type the guest imports or receives; a new export
/// or a new function in an exported interface is one an older guest does not provide.
fn breaking_lines(text: &str) -> Vec<&str> {
    let lines = wit_lines(text);
    let exported: Vec<&str> = lines
        .iter()
        .filter_map(|line| line.strip_prefix("export ")?.strip_suffix(';'))
        .collect();
    let mut breaking = Vec::new();
    let mut in_exported_interface = false;
    let mut in_type_block = false;
    for line in lines {
        if line.starts_with("export ") {
            breaking.push(line);
        } else if in_type_block {
            if line.starts_with('}') {
                in_type_block = false;
            } else {
                breaking.push(line);
            }
        } else if line.starts_with('}') {
            in_exported_interface = false;
        } else if in_exported_interface {
            breaking.push(line);
            in_type_block = line.ends_with('{');
        } else if let Some(name) = line
            .strip_prefix("interface ")
            .and_then(|rest| rest.strip_suffix(" {"))
        {
            in_exported_interface = exported.contains(&name);
        } else if line.ends_with('{')
            && ["variant ", "enum ", "record ", "flags "]
                .iter()
                .any(|keyword| line.starts_with(keyword))
        {
            in_type_block = true;
        }
    }
    breaking
}

fn diff_wit(old: &Value, new: &Value, changes: &mut Vec<Change>) {
    let (old, new) = (section(old, "wit"), section(new, "wit"));
    for (file, text) in old {
        let Some(replacement) = new.get(file) else {
            changes.push(breaking(format!("WIT file {file} removed")));
            continue;
        };
        let (before, after) = (
            wit_lines(text.as_str().unwrap()),
            wit_lines(replacement.as_str().unwrap()),
        );
        for line in before.iter().filter(|line| !after.contains(line)) {
            changes.push(breaking(format!("{file}: `{line}` removed or changed")));
        }
        let breaking_additions = breaking_lines(replacement.as_str().unwrap());
        for line in after.iter().filter(|line| !before.contains(line)) {
            let what = format!("{file}: `{line}` added");
            if breaking_additions.contains(line) {
                changes.push(breaking(what));
            } else {
                changes.push(additive(what));
            }
        }
    }
    for file in new.keys().filter(|file| !old.contains_key(*file)) {
        changes.push(additive(format!("WIT file {file} added")));
    }
}

fn diff_channels(old: &Value, new: &Value, changes: &mut Vec<Change>) {
    let (old, new) = (section(old, "channels"), section(new, "channels"));
    for (name, entry) in old {
        match new.get(name) {
            None => changes.push(breaking(format!("channel {name} removed"))),
            Some(replacement) if replacement != entry => {
                changes.push(breaking(format!("channel {name} changed")));
            }
            Some(_) => {}
        }
    }
    for name in new.keys().filter(|name| !old.contains_key(*name)) {
        changes.push(additive(format!("channel {name} added")));
    }
}

/// A new variant fails to decode in a plugin built against the older enum; a new
/// struct field is safe only when an existing plugin's payload may omit it.
fn added_member_breaks(definition: &Value, declaration: &str) -> bool {
    match definition["kind"].as_str() {
        Some("enum") => true,
        Some("struct") => {
            let defaulted = declaration.contains("serde(default")
                || declaration.contains(": Option<")
                || definition["attributes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|attribute| attribute.as_str().unwrap().contains("default"));
            !defaulted
        }
        _ => false,
    }
}

fn diff_types(old: &Value, new: &Value, changes: &mut Vec<Change>) {
    let (old, new) = (section(old, "types"), section(new, "types"));
    for (name, definition) in new {
        let Some(before) = old.get(name) else {
            changes.push(additive(format!("type {name} added")));
            continue;
        };
        for key in ["kind", "attributes"] {
            if before[key] != definition[key] {
                changes.push(breaking(format!("type {name}: {key} changed")));
            }
        }
        let (before, after) = (
            before["members"].as_object().unwrap(),
            definition["members"].as_object().unwrap(),
        );
        for (member, declaration) in before {
            match after.get(member) {
                None => changes.push(breaking(format!("type {name}: `{member}` removed"))),
                Some(now) if now != declaration => {
                    changes.push(breaking(format!("type {name}: `{member}` changed")));
                }
                Some(_) => {}
            }
        }
        for (member, declaration) in after
            .iter()
            .filter(|(member, _)| !before.contains_key(*member))
        {
            let what = format!("type {name}: `{member}` added");
            if added_member_breaks(definition, declaration.as_str().unwrap()) {
                changes.push(breaking(what));
            } else {
                changes.push(additive(what));
            }
        }
    }
}

fn diff(old: &Value, new: &Value) -> Vec<Change> {
    let mut changes = Vec::new();
    diff_wit(old, new, &mut changes);
    diff_channels(old, new, &mut changes);
    diff_types(old, new, &mut changes);
    changes
}

fn render(changes: &[Change]) -> String {
    changes
        .iter()
        .map(|change| {
            let label = if change.breaking {
                "breaking"
            } else {
                "additive"
            };
            format!("  {label}: {}", change.what)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn golden_path() -> PathBuf {
    crate_dir().join(GOLDEN)
}

fn write_snapshot(path: &Path, snapshot: &Value) {
    fs::write(path, serde_json::to_string_pretty(snapshot).unwrap() + "\n").unwrap();
}

#[test]
fn plugin_api_matches_its_snapshot() {
    let current = current_snapshot();
    let path = golden_path();
    let updating = std::env::var_os("HESTIA_UPDATE_GOLDEN").is_some();
    if updating && !path.exists() {
        write_snapshot(&path, &current);
        return;
    }
    let golden: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    if golden == current {
        return;
    }
    let changes = diff(&golden, &current);
    let listing = render(&changes);
    let regenerate =
        "regenerate with `HESTIA_UPDATE_GOLDEN=1 cargo test -p proto --test plugin_api`";
    let (old, new) = (version(&golden), version(&current));
    if changes.iter().any(|change| change.breaking) && new.0 <= old.0 {
        panic!(
            "breaking plugin API change without a major bump: bump the major of the WIT package \
             (and `proto::plugin::API_VERSION`), then {regenerate}\n{listing}"
        );
    }
    if (new.0, new.1) <= (old.0, old.1) {
        panic!(
            "additive plugin API change without a minor bump: bump the minor of the WIT package \
             (and `proto::plugin::API_VERSION`) so older hosts refuse plugins that need this, \
             then {regenerate}\n{listing}"
        );
    }
    if updating {
        write_snapshot(&path, &current);
        return;
    }
    panic!("the plugin API changed and its snapshot is stale; {regenerate}\n{listing}");
}

fn version(snapshot: &Value) -> (u64, u64) {
    let parts: Vec<u64> = snapshot["apiVersion"]
        .as_str()
        .expect("apiVersion is a string")
        .split('.')
        .map(|part| part.parse().expect("apiVersion is `N.M.P`"))
        .collect();
    (parts[0], parts[1])
}

#[test]
fn only_a_member_old_payloads_can_omit_is_additive() {
    let snapshot = |kind: &str, members: Value| {
        json!({
            "wit": {},
            "channels": {},
            "types": { "Renamed": { "kind": kind, "attributes": [], "members": members } },
        })
    };
    let before = snapshot(
        "struct",
        json!({ "id": "pub id: String", "name": "pub name: String" }),
    );
    let after = snapshot(
        "struct",
        json!({
            "id": "pub id: String",
            "title": "pub title: String",
            "note": "pub note: Option<String>",
            "tags": "serde(default) pub tags: Vec<String>",
        }),
    );
    let verdicts: Vec<(bool, String)> = diff(&before, &after)
        .into_iter()
        .map(|change| (change.breaking, change.what))
        .collect();
    assert_eq!(
        verdicts,
        [
            (true, "type Renamed: `name` removed".to_string()),
            (false, "type Renamed: `note` added".to_string()),
            (false, "type Renamed: `tags` added".to_string()),
            (true, "type Renamed: `title` added".to_string()),
        ]
    );

    let variants = |members: Value| snapshot("enum", members);
    let changes = diff(
        &variants(json!({ "A": "A" })),
        &variants(json!({ "A": "A", "B": "B" })),
    );
    assert!(changes[0].breaking);
}

#[test]
fn only_an_import_the_host_adds_is_additive_in_wit() {
    let snapshot = |wit: &str| json!({ "wit": { "plugin.wit": wit } });
    let base = "interface host {\ncall: func();\n}\ninterface lifecycle {\ninit: func();\n}\nrecord point {\nx: u32,\n}\nworld plugin {\nimport host;\nexport lifecycle;\n}";
    let verdict = |old: &str, new: &str| {
        let mut changes = Vec::new();
        diff_wit(&snapshot(old), &snapshot(new), &mut changes);
        changes.iter().all(|change| change.breaking)
    };
    let with = |anchor: &str, extra: &str| base.replacen(anchor, &format!("{anchor}\n{extra}"), 1);

    assert!(!verdict(base, &with("interface host {", "run: func();")));
    assert!(verdict(
        base,
        &with("interface lifecycle {", "shutdown: func();")
    ));
    assert!(verdict(base, &with("export lifecycle;", "export other;")));
    assert!(verdict(base, &with("record point {", "y: u32,")));
}
