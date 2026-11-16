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

    /// `hestia:kernel`. Only `secrets` asks: everything else stays inside the
    /// plugin's own call, files and dialogs Hestia draws.
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
                ("hestia:kernel/lifecycle", None),
                ("hestia:kernel/settings-changed", None),
            ],
            vec![Permission {
                name: "secrets",
                tier: Tier::Sandboxed,
                scope: None,
                prompt: "Keep secrets you enter for it, such as tokens",
            }],
        )
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
