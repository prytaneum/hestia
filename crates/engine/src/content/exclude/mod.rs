//! Which of a modpack's index files a **server** actually takes, and which of
//! its override files are written.
//!
//! A pack's `env.server` is the author's claim, and packs get it wrong
//! constantly — Aged 3.1.2 marks sodium, iris, entityculling and eleven other
//! client mods as `server: required`. Installing those is how a modpack "works"
//! on a client and breaks on a server, so the claim needs a correction layer
//! over it rather than blind trust.
//!
//! The rule is itzg/docker-minecraft-server's `FileInclusionCalculator`, and
//! [`defaults`] is its list:
//!
//! ```text
//! include = force_include(path) || (env.server != unsupported && !exclude(path))
//! ```
//!
//! Matching is a case-insensitive substring of the file's whole path, so
//! `"sodium"` catches `mods/sodium-fabric-0.5.11+mc1.20.1.jar` without anyone
//! writing a version. A force-include outranks everything, `env.server` included
//! — that is the escape hatch for a pack that under-declares.
//!
//! **Server-side only.** The list is "client mods a pack wrongly called
//! server-compatible"; applying it to an instance would strip sodium and iris
//! out of every modpack a user installs.

mod defaults;

/// The corrections in force for one pack install: the shipped table (unless
/// switched off), that table's per-pack entry, and the user's own additions.
#[derive(Default, Debug)]
pub struct Inclusion {
    excludes: Vec<String>,
    force_includes: Vec<String>,
}

impl Inclusion {
    /// Nothing corrected — every `env.server` claim taken at face value. This is
    /// the client side, and the server side with the defaults switched off and
    /// no user list.
    pub fn none() -> Self {
        Inclusion::default()
    }

    /// `slug` selects the shipped table's per-pack entry; a pack read off disk
    /// has none and simply takes the global rules. `defaults` false drops the
    /// shipped table entirely, leaving only what the user listed.
    pub fn new(slug: &str, defaults: bool, excludes: &str, force_includes: &str) -> Self {
        let mut inclusion = Inclusion {
            excludes: parse_list(excludes),
            force_includes: parse_list(force_includes),
        };
        if !defaults {
            return inclusion;
        }
        inclusion.excludes.extend(owned(defaults::GLOBAL_EXCLUDES));
        inclusion
            .force_includes
            .extend(owned(defaults::GLOBAL_FORCE_INCLUDES));
        if let Some(rules) = defaults::PACKS.iter().find(|p| p.slug == slug) {
            inclusion.excludes.extend(owned(rules.excludes));
            inclusion.force_includes.extend(owned(rules.force_includes));
        }
        inclusion
    }

    /// Whether the pack file at `path` is installed. `wanted` is what the pack's
    /// own `env` says for this side.
    pub fn includes(&self, path: &str, wanted: bool) -> bool {
        if matches(&self.force_includes, path) {
            return true;
        }
        wanted && !matches(&self.excludes, path)
    }
}

/// A comma- or newline-delimited user list, `#` starting a comment — the same
/// shape itzg's `MODRINTH_EXCLUDE_FILES` accepts, so a docker-mc-server user's
/// list pastes straight into the config key.
fn parse_list(raw: &str) -> Vec<String> {
    raw.split(['\n', ','])
        .map(|entry| entry.split('#').next().unwrap_or_default().trim())
        .filter(|entry| !entry.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn owned(entries: &'static [&'static str]) -> impl Iterator<Item = String> {
    entries.iter().map(|e| e.to_string())
}

fn matches(patterns: &[String], path: &str) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let path = path.replace('\\', "/").to_lowercase();
    patterns.iter().any(|pattern| path.contains(pattern))
}

/// Ant-style path patterns excluding files from a pack's override trees
/// (itzg's `--overrides-exclusions`): `?` one character, `*` any run within one
/// segment, `**` any run across segments. Matched against the path relative to
/// the game directory.
#[derive(Default, Debug)]
pub struct OverridePatterns {
    patterns: Vec<String>,
}

impl OverridePatterns {
    pub fn new(raw: &str) -> Self {
        OverridePatterns {
            patterns: parse_list(raw),
        }
    }

    pub fn excludes(&self, path: &str) -> bool {
        let path = path.replace('\\', "/").to_lowercase();
        self.patterns.iter().any(|p| ant_matches(p, &path))
    }
}

/// Ant matching over the two strings' segments. `**` consumes any number of
/// segments, so it is the one case needing backtracking; everything else is a
/// segment-against-segment glob.
fn ant_matches(pattern: &str, path: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    ant_segments(&pattern, &path)
}

fn ant_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.first() {
        None => path.is_empty(),
        Some(&"**") => (0..=path.len()).any(|skip| ant_segments(&pattern[1..], &path[skip..])),
        Some(head) => match path.first() {
            Some(segment) if glob_matches(head, segment) => ant_segments(&pattern[1..], &path[1..]),
            _ => false,
        },
    }
}

/// One segment against one segment: `*` any run of characters, `?` exactly one.
fn glob_matches(pattern: &str, segment: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let segment: Vec<char> = segment.chars().collect();
    let (mut p, mut s) = (0, 0);
    let (mut star, mut resume) = (None, 0);
    while s < segment.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some(p);
                resume = s;
                p += 1;
            }
            Some('?') => {
                p += 1;
                s += 1;
            }
            Some(c) if *c == segment[s] => {
                p += 1;
                s += 1;
            }
            _ => match star {
                Some(at) => {
                    p = at + 1;
                    resume += 1;
                    s = resume;
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four Aged 3.1.2 declares `server: required` that actually loaded on
    /// a tester's server — the regression this layer exists for. (Its other ten
    /// mislabelled mods were skipped by Fabric's own environment check.)
    #[test]
    fn the_mods_that_reached_a_server_are_held_back() {
        let inclusion = Inclusion::new("aged", true, "", "");
        for file in [
            "mods/entityculling-fabric-1.7.1-mc1.20.1.jar",
            "mods/lootbeams-2.1.1+1.20.1.jar",
            "mods/skinlayers3d-fabric-1.7.2-mc1.20.1.jar",
            "mods/welcomescreen-1.0.1.jar",
        ] {
            assert!(!inclusion.includes(file, true), "{file} is client-only");
        }
    }

    #[test]
    fn a_per_pack_rule_applies_only_to_that_pack() {
        assert!(!Inclusion::new("cobbleverse", true, "", "")
            .includes("mods/cloth-config-11.1.136.jar", true));
        assert!(
            Inclusion::new("aged", true, "", "").includes("mods/cloth-config-11.1.136.jar", true)
        );
    }

    #[test]
    fn a_force_include_outranks_both_the_table_and_the_packs_env() {
        let inclusion = Inclusion::new("", true, "", "sodium");
        assert!(inclusion.includes("mods/sodium-fabric-0.5.11.jar", true));
        assert!(
            inclusion.includes("mods/sodium-fabric-0.5.11.jar", false),
            "a force-include reaches a file the pack marked unsupported"
        );
    }

    #[test]
    fn an_unsupported_file_stays_out_without_a_force_include() {
        assert!(!Inclusion::none().includes("mods/modmenu-7.2.2.jar", false));
        assert!(Inclusion::none().includes("mods/modmenu-7.2.2.jar", true));
    }

    #[test]
    fn matching_is_case_insensitive_across_the_whole_path() {
        let inclusion = Inclusion::new("", false, "SODIUM", "");
        assert!(!inclusion.includes("mods/Sodium-Fabric.jar", true));
        assert!(!inclusion.includes("mods\\Sodium-Fabric.jar", true));
    }

    #[test]
    fn a_user_list_takes_commas_newlines_and_comments() {
        let inclusion = Inclusion::new("", false, "alpha, beta # why\n gamma", "");
        assert_eq!(inclusion.excludes, ["alpha", "beta", "gamma"]);
        assert!(!inclusion.includes("mods/beta-1.0.jar", true));
    }

    #[test]
    fn switching_the_defaults_off_leaves_only_the_users_list() {
        let inclusion = Inclusion::new("", false, "lithium", "");
        assert!(inclusion.includes("mods/sodium-fabric-0.5.11.jar", true));
        assert!(!inclusion.includes("mods/lithium-fabric-0.11.2.jar", true));
    }

    #[test]
    fn ant_patterns_match_segments_and_spans() {
        let patterns = OverridePatterns::new("config/**, *.txt, mods/?.jar");
        assert!(patterns.excludes("config/sodium/options.json"));
        assert!(patterns.excludes("config/one.toml"));
        assert!(patterns.excludes("options.txt"));
        assert!(patterns.excludes("mods/a.jar"));
        assert!(!patterns.excludes("mods/sodium.jar"));
        assert!(!patterns.excludes("resources/pack.png"));
        assert!(
            !patterns.excludes("deep/options.txt"),
            "a single * does not cross a segment boundary"
        );
    }
}
