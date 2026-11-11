//! Checking a built component against the plugin world this CLI was compiled with.

use std::collections::BTreeSet;

use anyhow::{bail, Context, Result};
use wasmparser::{Parser, Payload};

const PLUGIN_NAMESPACE: &str = "hestia:plugin/";
const WORLD: &str = include_str!(concat!(env!("OUT_DIR"), "/world.txt"));

/// The `hestia:plugin/*` interfaces a component imports and exports.
#[derive(Debug, Default, PartialEq, Eq)]
struct Interfaces {
    imports: BTreeSet<String>,
    exports: BTreeSet<String>,
}

impl Interfaces {
    fn of_world() -> Interfaces {
        let mut world = Interfaces::default();
        for line in WORLD.lines() {
            match line.split_once(' ') {
                Some(("import", name)) => world.imports.insert(name.to_string()),
                Some(("export", name)) => world.exports.insert(name.to_string()),
                _ => continue,
            };
        }
        world
    }

    fn of_component(wasm: &[u8]) -> Result<Interfaces> {
        let mut found = Interfaces::default();
        let mut depth = 0usize;
        for payload in Parser::new(0).parse_all(wasm) {
            match payload.context("the component is not valid WebAssembly")? {
                Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => depth += 1,
                Payload::End(_) => depth = depth.saturating_sub(1),
                Payload::ComponentImportSection(reader) if depth == 0 => {
                    for import in reader {
                        found.imports.insert(import?.name.name.to_string());
                    }
                }
                Payload::ComponentExportSection(reader) if depth == 0 => {
                    for export in reader {
                        found.exports.insert(export?.name.name.to_string());
                    }
                }
                _ => {}
            }
        }
        found
            .imports
            .retain(|name| name.starts_with(PLUGIN_NAMESPACE));
        found
            .exports
            .retain(|name| name.starts_with(PLUGIN_NAMESPACE));
        Ok(found)
    }

    fn outside(&self, world: &Interfaces) -> Vec<String> {
        let exports = self
            .exports
            .difference(&world.exports)
            .map(|name| format!("exports {name}"));
        let imports = self
            .imports
            .difference(&world.imports)
            .map(|name| format!("imports {name}"));
        exports.chain(imports).collect()
    }
}

/// Refuses a component that exports a `hestia:plugin/*` interface the world
/// does not export, or imports one it does not provide. `wasi:*` is left to
/// the host's own linkage, and a world export the component omits is allowed.
pub(super) fn check_component(plugin_id: &str, wasm: &[u8]) -> Result<()> {
    let offending = Interfaces::of_component(wasm)?.outside(&Interfaces::of_world());
    if offending.is_empty() {
        return Ok(());
    }
    bail!(
        "the component of '{plugin_id}' was built against a different plugin API than this hestia ({}); run `cargo update -p hestia-sdk` and rebuild",
        offending.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use wasm_encoder::{
        Component, ComponentImportSection, ComponentTypeRef, ComponentTypeSection, InstanceType,
    };

    use super::*;

    fn interfaces(imports: &[&str], exports: &[&str]) -> Interfaces {
        Interfaces {
            imports: imports.iter().map(|name| name.to_string()).collect(),
            exports: exports.iter().map(|name| name.to_string()).collect(),
        }
    }

    fn component_importing(name: &str) -> Vec<u8> {
        let mut types = ComponentTypeSection::new();
        types.instance(&InstanceType::new());
        let mut imports = ComponentImportSection::new();
        imports.import(name, ComponentTypeRef::Instance(0));
        let mut component = Component::new();
        component.section(&types).section(&imports);
        component.finish()
    }

    #[test]
    fn the_world_comes_from_the_wit() {
        let world = Interfaces::of_world();
        assert!(world.exports.contains("hestia:plugin/hooks@1.0.0"));
        assert!(world.imports.contains("hestia:plugin/host@1.0.0"));
    }

    #[test]
    fn an_old_export_and_an_unknown_import_are_outside_the_world() {
        let world = interfaces(
            &["hestia:plugin/host@1.0.0"],
            &["hestia:plugin/hooks@1.0.0"],
        );
        let component = interfaces(
            &["hestia:plugin/host@1.0.0", "hestia:plugin/gone@1.0.0"],
            &["hestia:plugin/launch-hooks@1.0.0"],
        );
        assert_eq!(
            component.outside(&world),
            [
                "exports hestia:plugin/launch-hooks@1.0.0",
                "imports hestia:plugin/gone@1.0.0"
            ]
        );
    }

    #[test]
    fn omitted_exports_are_fine() {
        let world = interfaces(&[], &["hestia:plugin/hooks@1.0.0"]);
        assert!(interfaces(&[], &[]).outside(&world).is_empty());
    }

    #[test]
    fn a_component_using_an_interface_outside_the_world_is_refused() {
        let wasm = component_importing("hestia:plugin/gone@1.0.0");
        let error = check_component("tunnel", &wasm).unwrap_err().to_string();
        assert!(error.contains("'tunnel'") && error.contains("imports hestia:plugin/gone@1.0.0"));
        assert!(error.contains("cargo update -p hestia-sdk"));
    }

    #[test]
    fn wasi_and_current_plugin_interfaces_pass() {
        assert!(check_component("tunnel", &component_importing("wasi:io/streams@0.2.9")).is_ok());
        assert!(
            check_component("tunnel", &component_importing("hestia:plugin/host@1.0.0")).is_ok()
        );
    }

    #[test]
    fn a_component_built_from_the_current_sdk_passes() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../engine/tests/fixture/plugins/probe/Cargo.toml");
        let target = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(env!("CARGO"))
            .args([
                "build",
                "--release",
                "--target",
                "wasm32-wasip2",
                "--manifest-path",
            ])
            .arg(&manifest)
            .arg("--target-dir")
            .arg(target.path())
            .status()
            .unwrap();
        assert!(status.success(), "is the wasm32-wasip2 target installed?");
        let wasm = std::fs::read(target.path().join("wasm32-wasip2/release/probe.wasm")).unwrap();

        assert!(Interfaces::of_component(&wasm)
            .unwrap()
            .exports
            .contains("hestia:plugin/lifecycle@1.0.0"));
        check_component("probe", &wasm).unwrap();
    }
}
