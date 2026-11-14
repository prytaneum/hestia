//! The plugin guide's compiled examples. A fenced block tagged
//! `fixture=<path>` is an excerpt of that file: every line of it is a line of
//! the file, in order, and a line that is only `...` stands for the lines left
//! out. The fixtures are built and run by the other tests, so an example in the
//! guide cannot drift from code that compiles.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Excerpt {
    page: String,
    fixture: PathBuf,
    lines: Vec<String>,
}

fn tag(info: &str) -> Option<&str> {
    info.split_whitespace()
        .find_map(|word| word.strip_prefix("fixture="))
}

fn excerpts() -> Vec<Excerpt> {
    let mut found = Vec::new();
    let dir = root().join("docs/plugins");
    for entry in fs::read_dir(&dir).expect("the guide") {
        let path = entry.expect("an entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let page = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = fs::read_to_string(&path).expect("read a page");
        let mut inside = false;
        let mut current: Option<(PathBuf, Vec<String>)> = None;
        for line in text.lines() {
            if let Some(info) = line.strip_prefix("```") {
                if inside {
                    if let Some((fixture, lines)) = current.take() {
                        found.push(Excerpt {
                            page: page.clone(),
                            fixture,
                            lines,
                        });
                    }
                } else {
                    current = tag(info).map(|fixture| (root().join(fixture), Vec::new()));
                }
                inside = !inside;
            } else if let Some((_, lines)) = &mut current {
                lines.push(line.trim_end().to_string());
            }
        }
    }
    found
}

fn missing_line(excerpt: &Excerpt) -> Option<&str> {
    let source = fs::read_to_string(&excerpt.fixture)
        .unwrap_or_else(|_| panic!("{}: no file {}", excerpt.page, excerpt.fixture.display()));
    let mut remaining = source.lines().map(str::trim_end);
    excerpt
        .lines
        .iter()
        .filter(|line| !line.is_empty() && line.trim() != "...")
        .map(String::as_str)
        .find(|line| !remaining.any(|candidate| candidate == *line))
}

#[test]
fn a_guide_example_is_made_of_the_lines_of_the_fixture_it_names() {
    let all = excerpts();
    assert!(!all.is_empty(), "the guide tags no excerpt");
    for excerpt in &all {
        if let Some(line) = missing_line(excerpt) {
            panic!(
                "{} shows a line that is not in {} (or is out of order): {line}",
                excerpt.page,
                excerpt.fixture.display()
            );
        }
    }
}

fn plugin_manifest(fixture: &Path) -> String {
    let manifest = fixture
        .ancestors()
        .map(|dir| dir.join("plugin.toml"))
        .find(|manifest| manifest.is_file())
        .unwrap_or_else(|| panic!("{} is in no plugin", fixture.display()));
    fs::read_to_string(manifest).expect("read the manifest")
}

#[test]
fn every_provider_kind_has_a_compiled_example_in_the_guide() {
    let all = excerpts();
    for spec in proto::provider::KINDS {
        let declared = format!("kind = \"{}\"", spec.kind);
        let shown = all.iter().any(|excerpt| {
            excerpt.page == "providers.md"
                && excerpt.fixture.extension().is_some_and(|ext| ext == "rs")
                && plugin_manifest(&excerpt.fixture).contains(&declared)
        });
        assert!(
            shown,
            "providers.md has no compiled example of `{}`",
            spec.kind
        );
    }
}
