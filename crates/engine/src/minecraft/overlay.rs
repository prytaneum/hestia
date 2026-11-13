//! Layering a flavor's additions over the vanilla profile: how a modloader's
//! version manifest merges over the game's it inherits from.

use std::collections::HashMap;

use proto::profile::ServerJar;

use super::maven::Coordinate;
use super::profile::{InstanceProfile, Library, ServerProfile};

/// What a flavor adds to the client, resolved: libraries complete and
/// arguments already filtered for this machine.
#[derive(Debug, Default)]
pub(crate) struct ClientLayer {
    pub loader_version: Option<String>,
    pub libraries: Vec<Library>,
    pub main_class: String,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
}

/// What a flavor adds to the server, resolved.
#[derive(Debug, Default)]
pub(crate) struct ServerLayer {
    pub loader_version: Option<String>,
    pub jar: Option<ServerJar>,
    pub jvm_args: Vec<String>,
}

pub(crate) fn client(base: InstanceProfile, addition: ClientLayer) -> InstanceProfile {
    let mut jvm_args = base.jvm_args;
    jvm_args.extend(addition.jvm_args);
    let mut game_args = base.game_args;
    game_args.extend(addition.game_args);
    InstanceProfile {
        loader_version: addition.loader_version,
        libraries: merge_libraries(base.libraries, addition.libraries),
        main_class: if addition.main_class.is_empty() {
            base.main_class
        } else {
            addition.main_class
        },
        jvm_args,
        game_args,
        ..base
    }
}

pub(crate) fn server(base: ServerProfile, addition: ServerLayer) -> ServerProfile {
    ServerProfile {
        loader_version: addition.loader_version,
        jar: addition.jar.unwrap_or(base.jar),
        jvm_args: addition.jvm_args,
        ..base
    }
}

/// Layer a modloader's libraries over the vanilla base the way an inheriting
/// version manifest merges: an artifact the loader re-pins (commonly
/// `org.ow2.asm:asm`, which vanilla and the loader carry at different versions)
/// resolves to a single entry — two versions of one artifact on the classpath
/// make Fabric's loader abort ("duplicate ASM classes found on classpath").
/// Keyed by the coordinate minus its version, so a jar and its `natives-*`
/// sibling (same artifact, different classifier) both survive; the overlay wins
/// a collision and first-seen order is preserved.
pub(crate) fn merge_libraries(base: Vec<Library>, overlay: Vec<Library>) -> Vec<Library> {
    let mut order: Vec<String> = Vec::new();
    let mut by_key: HashMap<String, Library> = HashMap::new();
    for library in base.into_iter().chain(overlay) {
        let key = versionless_key(&library.name);
        if by_key.insert(key.clone(), library).is_none() {
            order.push(key);
        }
    }
    order
        .into_iter()
        .filter_map(|key| by_key.remove(&key))
        .collect()
}

/// A Maven coordinate (`group:artifact:version[:classifier][@ext]`) reduced to
/// its identity ignoring version. Falls back to the raw coordinate when it does
/// not parse, so an unrecognised entry is never merged into an unrelated one.
fn versionless_key(coord: &str) -> String {
    let Ok(parsed) = Coordinate::parse(coord) else {
        return coord.to_string();
    };
    match parsed.classifier {
        Some(classifier) => format!(
            "{}:{}:{classifier}@{}",
            parsed.group, parsed.artifact, parsed.extension
        ),
        None => format!("{}:{}@{}", parsed.group, parsed.artifact, parsed.extension),
    }
}

#[cfg(test)]
mod tests {
    use proto::minecraft::Artifact;

    use super::*;

    fn lib(name: &str) -> Library {
        Library {
            name: name.to_string(),
            path: name.to_string(),
            artifact: Artifact::default(),
            ..Library::default()
        }
    }

    fn names(libs: &[Library]) -> Vec<&str> {
        libs.iter().map(|l| l.name.as_str()).collect()
    }

    #[test]
    fn overlay_repin_collapses_to_one_version() {
        let merged = merge_libraries(
            vec![
                lib("org.ow2.asm:asm:9.6"),
                lib("com.google.guava:guava:32.1"),
            ],
            vec![lib("org.ow2.asm:asm:9.10.1")],
        );
        assert_eq!(
            names(&merged),
            ["org.ow2.asm:asm:9.10.1", "com.google.guava:guava:32.1"],
            "the overlay version wins in the base's slot"
        );
    }

    #[test]
    fn distinct_classifiers_are_kept() {
        let merged = merge_libraries(
            vec![
                lib("org.lwjgl:lwjgl:3.3.3"),
                lib("org.lwjgl:lwjgl:3.3.3:natives-linux"),
            ],
            Vec::new(),
        );
        assert_eq!(
            names(&merged).len(),
            2,
            "jar and its natives sibling survive"
        );
    }
}
