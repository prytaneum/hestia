use std::path::PathBuf;
use std::{env, fs};

use wit_parser::{Resolve, WorldItem};

const WIT_DIR: &str = "../sdk/wit";
const WORLD: &str = "plugin";

fn main() {
    println!("cargo:rerun-if-changed={WIT_DIR}");
    let mut resolve = Resolve::new();
    let (package, _) = resolve.push_dir(WIT_DIR).expect("the SDK's WIT parses");
    let world = resolve
        .select_world(&[package], Some(WORLD))
        .expect("the SDK declares the plugin world");

    let mut lines = Vec::new();
    for (direction, items) in [
        ("import", &resolve.worlds[world].imports),
        ("export", &resolve.worlds[world].exports),
    ] {
        for item in items.values() {
            if let WorldItem::Interface { id, .. } = item {
                if let Some(name) = resolve.id_of(*id) {
                    lines.push(format!("{direction} {name}"));
                }
            }
        }
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("world.txt");
    fs::write(out, lines.join("\n")).unwrap();
}
