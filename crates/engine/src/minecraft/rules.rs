//! Mojang-style `rules` against the machine: the one evaluator behind the game's
//! own libraries and arguments and a plugin's, and the host facts a plugin is
//! told so it never probes them.

use proto::provider::flavor::Target;
use proto::rules::{Rule, RuleAction};
use serde_json::Value;

/// Host OS in Mojang's rule vocabulary (`os.name`).
pub(crate) fn host_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

/// Host architecture in the vocabulary of a rule's `os.arch`.
pub(crate) fn host_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        std::env::consts::ARCH
    }
}

/// What a plugin is told of this machine; `java_major` is the one the game will
/// run on, when the caller knows it.
pub(crate) fn host_target(java_major: Option<i32>) -> Target {
    Target {
        os: host_os().to_string(),
        arch: host_arch().to_string(),
        java_major: java_major.filter(|major| *major > 0),
        path_separator: if cfg!(windows) { ";" } else { ":" }.to_string(),
    }
}

/// Whether `rules` allow something on `target`. No rules allows it. Otherwise
/// the last rule that applies decides, and nothing allows it until one does. A
/// rule applies when its `os` matches (a missing `name` or `arch` matches any);
/// one that names `features` is skipped, since no feature is enabled.
pub(crate) fn allow(rules: &[Rule], target: &Target) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule.features.is_some() {
            continue;
        }
        let matches = rule.os.as_ref().is_none_or(|os| {
            os.name.as_deref().is_none_or(|name| name == target.os)
                && os.arch.as_deref().is_none_or(|arch| arch == target.arch)
        });
        if matches {
            allowed = rule.action == RuleAction::Allow;
        }
    }
    allowed
}

/// [`allow`] for the `rules` array of a Mojang manifest on this machine. A
/// value that is not a list of rules does not gate anything.
pub(crate) fn allow_manifest(rules: &Value) -> bool {
    match serde_json::from_value::<Vec<Rule>>(rules.clone()) {
        Ok(rules) => allow(&rules, &host_target(None)),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use proto::rules::OsRule;
    use serde_json::json;

    use super::*;

    fn on(os: &str, arch: &str) -> Target {
        Target {
            os: os.into(),
            arch: arch.into(),
            ..Target::default()
        }
    }

    fn rule(action: RuleAction, os: Option<&str>, arch: Option<&str>) -> Rule {
        Rule {
            action,
            os: (os.is_some() || arch.is_some()).then(|| OsRule {
                name: os.map(str::to_string),
                arch: arch.map(str::to_string),
                version: None,
            }),
            features: None,
        }
    }

    #[test]
    fn no_rules_allow() {
        assert!(allow(&[], &on("linux", "x86_64")));
    }

    #[test]
    fn an_allow_for_one_os_allows_only_that_os() {
        let rules = [rule(RuleAction::Allow, Some("osx"), None)];
        assert!(allow(&rules, &on("osx", "arm64")));
        assert!(!allow(&rules, &on("linux", "x86_64")));
        assert!(!allow(&rules, &on("windows", "x86_64")));
    }

    #[test]
    fn a_disallow_after_an_allow_carves_an_exception() {
        let rules = [
            rule(RuleAction::Allow, None, None),
            rule(RuleAction::Disallow, Some("osx"), None),
        ];
        assert!(allow(&rules, &on("linux", "x86_64")));
        assert!(allow(&rules, &on("windows", "x86_64")));
        assert!(!allow(&rules, &on("osx", "arm64")));
    }

    #[test]
    fn the_last_applicable_rule_wins() {
        let rules = [
            rule(RuleAction::Disallow, Some("linux"), None),
            rule(RuleAction::Allow, Some("linux"), None),
        ];
        assert!(allow(&rules, &on("linux", "x86_64")));
    }

    #[test]
    fn an_architecture_narrows_a_rule() {
        let rules = [rule(RuleAction::Allow, Some("linux"), Some("arm64"))];
        assert!(allow(&rules, &on("linux", "arm64")));
        assert!(!allow(&rules, &on("linux", "x86_64")));
        assert!(!allow(&rules, &on("windows", "arm64")));
        let any_os = [rule(RuleAction::Allow, None, Some("x86"))];
        assert!(allow(&any_os, &on("windows", "x86")));
        assert!(!allow(&any_os, &on("windows", "x86_64")));
    }

    #[test]
    fn a_rule_naming_features_is_skipped() {
        let featured = Rule {
            action: RuleAction::Allow,
            os: None,
            features: Some([("is_demo_user".to_string(), true)].into()),
        };
        assert!(!allow(
            std::slice::from_ref(&featured),
            &on("linux", "x86_64")
        ));
        let rules = [rule(RuleAction::Allow, Some("linux"), None), featured];
        assert!(allow(&rules, &on("linux", "x86_64")));
    }

    #[test]
    fn a_version_pattern_is_not_evaluated() {
        let rules = [Rule {
            action: RuleAction::Allow,
            os: Some(OsRule {
                name: Some("windows".into()),
                version: Some("^10\\.".into()),
                arch: None,
            }),
            features: None,
        }];
        assert!(allow(&rules, &on("windows", "x86_64")));
    }

    #[test]
    fn a_mojang_manifest_array_gates_by_the_host() {
        let host = host_os();
        let other = if host == "linux" { "windows" } else { "linux" };
        let only_here = json!([{ "action": "allow", "os": { "name": host } }]);
        let only_there = json!([{ "action": "allow", "os": { "name": other } }]);
        assert!(allow_manifest(&only_here));
        assert!(!allow_manifest(&only_there));
        assert!(allow_manifest(&json!([])));
        assert!(allow_manifest(&json!("nonsense")));
    }

    #[test]
    fn the_host_target_names_its_facts_in_the_rule_vocabulary() {
        let target = host_target(Some(21));
        assert!(["windows", "osx", "linux"].contains(&target.os.as_str()));
        assert!(!target.arch.is_empty());
        assert_eq!(target.java_major, Some(21));
        assert_eq!(target.path_separator, if cfg!(windows) { ";" } else { ":" });
        assert_eq!(host_target(Some(0)).java_major, None);
        assert_eq!(host_target(None).java_major, None);
    }
}
