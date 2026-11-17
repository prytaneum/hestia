use proto::plugin::{ScopeKind, Tier};

/// The version of `crates/sdk/wit/kernel`; the WIT snapshot test keeps the two equal.
const KERNEL_VERSION: &str = "0.1.0";

/// What the user is asked to approve for one permission.
#[derive(Debug, Clone, Copy)]
pub struct Permission {
    pub name: &'static str,
    pub tier: Tier,
    /// The manifest scope the permission requires, if it takes one;
    /// [`ScopeKind::Entries`] is chosen by the user instead.
    pub scope: Option<ScopeKind>,
    pub prompt: &'static str,
}

/// How far an interface's contract can be relied on, as its WIT says: a plain
/// interface is part of the package's baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stability {
    #[default]
    Baseline,
    /// `@since(version = ..)`, and `@deprecated(version = ..)` when it is on its way out.
    Stable {
        since: &'static str,
        deprecated: Option<&'static str>,
    },
    /// `@unstable(feature = ..)`: it may change in any release.
    Unstable { feature: &'static str },
}

/// What importing or exporting an interface needs.
pub enum Gate<'a> {
    Open,
    Needs(&'a Permission),
    Unknown,
}

/// One package's interfaces, each mapped to the permission it needs. A function
/// with no interface here is unreachable, so there is no denied list. Built-ins
/// add their package's table beside the kernel's.
#[derive(Debug, Clone, Default)]
pub struct Table {
    interfaces: Vec<(&'static str, Option<&'static str>)>,
    permissions: Vec<Permission>,
    versions: Vec<(&'static str, &'static str)>,
    stability: Vec<(&'static str, Stability)>,
}

impl Table {
    /// `interfaces` are `package/interface` names without a version; the
    /// permission of each is a name in `permissions`.
    pub fn new(
        interfaces: Vec<(&'static str, Option<&'static str>)>,
        permissions: Vec<Permission>,
    ) -> Self {
        Table {
            interfaces,
            permissions,
            ..Table::default()
        }
    }

    /// The version `package` is at; a plugin built against a later one is refused.
    pub fn versioned(mut self, package: &'static str, version: &'static str) -> Self {
        self.versions.push((package, version));
        self
    }

    /// Marks `interface` as stable since a version, deprecated, or unstable.
    pub fn stability(mut self, interface: &'static str, stability: Stability) -> Self {
        self.stability.push((interface, stability));
        self
    }

    /// The version `package` is at, if the table states one.
    pub fn version(&self, package: &str) -> Option<&'static str> {
        self.versions
            .iter()
            .find(|(name, _)| *name == package)
            .map(|(_, version)| *version)
    }

    /// What `interface` promises; a plain one is the package's baseline.
    pub fn stability_of(&self, interface: &str) -> Stability {
        self.stability
            .iter()
            .find(|(name, _)| *name == interface)
            .map_or(Stability::Baseline, |(_, stability)| *stability)
    }

    /// Every interface the table maps.
    pub fn interfaces(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.interfaces.iter().map(|(name, _)| *name)
    }

    /// Every interface the table marks with a stability.
    pub fn stabilities(&self) -> impl Iterator<Item = (&'static str, Stability)> + '_ {
        self.stability.iter().copied()
    }

    /// `hestia:kernel` and the `wasi:http` that rides on `network:connect`.
    /// Beyond `secrets` and the network, programs and local endpoints, nothing
    /// asks: the rest stays inside the plugin's own call, files and dialogs
    /// Hestia draws.
    pub fn kernel() -> Self {
        Table::new(
            vec![
                ("hestia:kernel/types", None),
                ("hestia:kernel/log", None),
                ("hestia:kernel/app", None),
                ("hestia:kernel/data", None),
                ("hestia:kernel/settings", None),
                ("hestia:kernel/permissions", None),
                ("hestia:kernel/secrets", Some("secrets")),
                ("hestia:kernel/messages", None),
                ("hestia:kernel/progress", None),
                ("hestia:kernel/notify", None),
                ("hestia:kernel/download", Some("network:connect")),
                ("hestia:kernel/programs", Some("process:spawn")),
                ("hestia:kernel/ipc", Some("ipc:connect")),
                ("wasi:http/types", Some("network:connect")),
                ("wasi:http/outgoing-handler", Some("network:connect")),
                ("hestia:kernel/lifecycle", None),
                ("hestia:kernel/settings-changed", None),
                ("hestia:kernel/jobs", None),
                ("hestia:kernel/commands", None),
                ("hestia:kernel/on-schedule", None),
                ("hestia:kernel/on-job-done", None),
            ],
            vec![
                Permission {
                    name: "secrets",
                    tier: Tier::Sandboxed,
                    scope: None,
                    prompt: "Keep secrets you enter for it, such as tokens",
                },
                Permission {
                    name: "network:connect",
                    tier: Tier::Sandboxed,
                    scope: Some(ScopeKind::Hosts),
                    prompt: "Connect to the listed hosts over https and download files from them",
                },
                Permission {
                    name: "process:spawn",
                    tier: Tier::Unrestricted,
                    scope: Some(ScopeKind::Programs),
                    prompt: "Run the listed programs on your computer, with the listed arguments",
                },
                Permission {
                    name: "ipc:connect",
                    tier: Tier::Unrestricted,
                    scope: Some(ScopeKind::Endpoints),
                    prompt:
                        "Talk to the listed local sockets and pipes, which other programs serve",
                },
            ],
        )
        .versioned(super::KERNEL_PACKAGE, KERNEL_VERSION)
    }

    /// Adds `package`'s table. Refused when a row lies outside `package`, an
    /// interface or permission is already in this table, or a row names a
    /// permission the table lacks: each would let one package's grant speak for
    /// another's.
    pub(crate) fn merge(&mut self, package: &str, other: Table) -> Result<(), String> {
        let prefix = format!("{package}/");
        for (interface, permission) in &other.interfaces {
            if !interface.starts_with(&prefix) {
                return Err(format!("'{interface}' is not an interface of {package}"));
            }
            if self.interfaces.iter().any(|(held, _)| held == interface) {
                return Err(format!("'{interface}' is in two tables"));
            }
            if let Some(name) = permission {
                if !other.permissions.iter().any(|p| p.name == *name) {
                    return Err(format!(
                        "'{interface}' needs '{name}', which {package} lacks"
                    ));
                }
            }
        }
        if let Some(taken) = other
            .permissions
            .iter()
            .find(|p| self.permissions.iter().any(|held| held.name == p.name))
        {
            return Err(format!("permission '{}' is in two tables", taken.name));
        }
        if other.version(package).is_none() {
            return Err(format!("{package} states no version"));
        }
        if let Some((name, _)) = other.versions.iter().find(|(name, _)| *name != package) {
            return Err(format!("'{name}' is not {package}"));
        }
        let unparsable = other
            .versions
            .iter()
            .map(|(_, version)| version)
            .chain(
                other
                    .stability
                    .iter()
                    .flat_map(|(_, stability)| match stability {
                        Stability::Stable { since, deprecated } => {
                            vec![since, deprecated.as_ref().unwrap_or(since)]
                        }
                        _ => Vec::new(),
                    }),
            )
            .find(|version| semver::Version::parse(version).is_err());
        if let Some(version) = unparsable {
            return Err(format!("'{version}' is not a version"));
        }
        if let Some((name, _)) = other
            .stability
            .iter()
            .find(|(name, _)| !other.interfaces.iter().any(|(held, _)| held == name))
        {
            return Err(format!("'{name}' is not an interface of {package}"));
        }
        self.interfaces.extend(other.interfaces);
        self.permissions.extend(other.permissions);
        self.versions.extend(other.versions);
        self.stability.extend(other.stability);
        Ok(())
    }

    pub fn knows_package(&self, package: &str) -> bool {
        self.interfaces
            .iter()
            .any(|(interface, _)| interface.split_once('/').is_some_and(|(p, _)| p == package))
    }

    pub fn gate(&self, interface: &str) -> Gate<'_> {
        let Some((_, permission)) = self.interfaces.iter().find(|(name, _)| *name == interface)
        else {
            return Gate::Unknown;
        };
        let Some(name) = permission else {
            return Gate::Open;
        };
        match self.permissions.iter().find(|p| p.name == *name) {
            Some(permission) => Gate::Needs(permission),
            None => Gate::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permission(name: &'static str) -> Permission {
        Permission {
            name,
            tier: Tier::Sandboxed,
            scope: None,
            prompt: "p",
        }
    }

    #[test]
    fn a_package_table_speaks_only_for_its_own_package() {
        let refused = [
            ("hestia:kernel/log", None, None, "the kernel's interface"),
            ("hestia:other/x", None, None, "another package's"),
            (
                "hestia:probe/x",
                Some("probe:x"),
                None,
                "a permission it lacks",
            ),
            (
                "hestia:probe/x",
                Some("secrets"),
                Some(permission("secrets")),
                "a kernel permission",
            ),
        ];
        for (interface, needs, extra, case) in refused {
            let mut table = Table::kernel();
            let other = Table::new(vec![(interface, needs)], extra.into_iter().collect())
                .versioned("hestia:probe", "0.1.0");
            assert!(table.merge("hestia:probe", other).is_err(), "{case}");
        }
        let read = || Table::new(vec![("hestia:probe/read", None)], vec![]);
        for (other, case) in [
            (read(), "no version"),
            (read().versioned("hestia:probe", "latest"), "not a version"),
            (
                read().versioned("hestia:probe", "0.1.0").stability(
                    "hestia:probe/read",
                    Stability::Stable {
                        since: "soon",
                        deprecated: None,
                    },
                ),
                "a since that is not a version",
            ),
        ] {
            assert!(
                Table::kernel().merge("hestia:probe", other).is_err(),
                "{case}"
            );
        }
        let mut table = Table::kernel();
        let probe = || {
            Table::new(
                vec![("hestia:probe/read", Some("probe:read"))],
                vec![permission("probe:read")],
            )
            .versioned("hestia:probe", "0.1.0")
        };
        table.merge("hestia:probe", probe()).unwrap();
        assert!(matches!(table.gate("hestia:probe/read"), Gate::Needs(_)));
        assert!(
            table.merge("hestia:probe", probe()).is_err(),
            "an interface in two tables"
        );
    }
}
