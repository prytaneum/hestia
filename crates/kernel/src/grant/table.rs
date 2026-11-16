use proto::plugin::{ScopeKind, Tier};

/// What the user is asked to approve for one permission.
#[derive(Debug, Clone, Copy)]
pub struct Permission {
    pub name: &'static str,
    pub tier: Tier,
    /// The manifest scope the permission requires, if it takes one.
    pub scope: Option<ScopeKind>,
    pub prompt: &'static str,
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
        }
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
        self.interfaces.extend(other.interfaces);
        self.permissions.extend(other.permissions);
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
            let other = Table::new(vec![(interface, needs)], extra.into_iter().collect());
            assert!(table.merge("hestia:probe", other).is_err(), "{case}");
        }
        let mut table = Table::kernel();
        let probe = || {
            Table::new(
                vec![("hestia:probe/read", Some("probe:read"))],
                vec![permission("probe:read")],
            )
        };
        table.merge("hestia:probe", probe()).unwrap();
        assert!(matches!(table.gate("hestia:probe/read"), Gate::Needs(_)));
        assert!(
            table.merge("hestia:probe", probe()).is_err(),
            "an interface in two tables"
        );
    }
}
