//! The runtime-mutable provider list shared by the content, Minecraft, Java and
//! pack aggregates. Every provider has an owner: the built-ins are `builtin`
//! and are never removed, a plugin's are registered as `<plugin-id>.<id>` and
//! leave with it.
//!
//! A caller may name a provider by its full id or by a bare name, and
//! [`Roster::resolve`] is the one place a bare name becomes a provider.

use std::sync::{Arc, RwLock};

use proto::error::ErrorInfo;

pub(crate) const BUILTIN_OWNER: &str = "builtin";

#[derive(Debug, thiserror::Error)]
pub enum RegisterError {
    #[error("`{0}` is reserved for the built-in providers")]
    ReservedOwner(String),
    #[error("provider id `{id}` must be namespaced as `{owner}.<id>`")]
    NotNamespaced { owner: String, id: String },
    #[error("a provider with id `{0}` is already registered")]
    Duplicate(String),
}

/// Why a name matched no single provider.
#[derive(Debug)]
pub(crate) enum Unresolved {
    Unknown(Vec<String>),
    Ambiguous(Vec<String>),
}

impl Unresolved {
    /// The typed failure for `name`, listing the full ids to choose from.
    pub(crate) fn into_error(self, name: &str) -> anyhow::Error {
        match self {
            Unresolved::Unknown(available) => ErrorInfo::ProviderUnknown {
                name: name.to_string(),
                available,
            }
            .into(),
            Unresolved::Ambiguous(candidates) => ErrorInfo::ProviderAmbiguous {
                name: name.to_string(),
                candidates,
            }
            .into(),
        }
    }
}

struct Entry<T: ?Sized> {
    owner: String,
    id: String,
    item: Arc<T>,
}

impl<T: ?Sized> Entry<T> {
    fn builtin(&self) -> bool {
        self.owner == BUILTIN_OWNER
    }

    fn local(&self) -> &str {
        self.id
            .strip_prefix(self.owner.as_str())
            .and_then(|rest| rest.strip_prefix('.'))
            .unwrap_or(&self.id)
    }
}

pub(crate) struct Roster<T: ?Sized> {
    entries: RwLock<Vec<Entry<T>>>,
}

impl<T: ?Sized> Roster<T> {
    pub(crate) fn builtin(items: impl IntoIterator<Item = (String, Arc<T>)>) -> Self {
        let entries = items
            .into_iter()
            .map(|(id, item)| Entry {
                owner: BUILTIN_OWNER.to_string(),
                id,
                item,
            })
            .collect();
        Roster {
            entries: RwLock::new(entries),
        }
    }

    /// `id` must be `<owner>.<local>`, so no plugin can take a built-in's id or
    /// another plugin's.
    pub(crate) fn register(
        &self,
        owner: &str,
        id: &str,
        item: Arc<T>,
    ) -> Result<(), RegisterError> {
        if owner == BUILTIN_OWNER {
            return Err(RegisterError::ReservedOwner(owner.to_string()));
        }
        let namespaced = id
            .strip_prefix(owner)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|local| !local.is_empty());
        if !namespaced {
            return Err(RegisterError::NotNamespaced {
                owner: owner.to_string(),
                id: id.to_string(),
            });
        }
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        if entries.iter().any(|e| e.id == id) {
            return Err(RegisterError::Duplicate(id.to_string()));
        }
        entries.push(Entry {
            owner: owner.to_string(),
            id: id.to_string(),
            item,
        });
        Ok(())
    }

    /// Drops everything `owner` registered; the built-ins stay whatever is asked.
    pub(crate) fn unregister(&self, owner: &str) -> usize {
        if owner == BUILTIN_OWNER {
            return 0;
        }
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        let before = entries.len();
        entries.retain(|e| e.owner != owner);
        before - entries.len()
    }

    /// A copy of the list, so a caller can await on a provider without holding
    /// the lock: the built-ins first, then the plugins' in registration order.
    pub(crate) fn snapshot(&self) -> Vec<Arc<T>> {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        entries.iter().map(|e| e.item.clone()).collect()
    }

    /// The provider `name` means among those `accepts` admits, with its full
    /// id. A full id means exactly itself. A bare name means the only
    /// plugin provider of that provider-id; two are ambiguous.
    pub(crate) fn resolve(
        &self,
        name: &str,
        accepts: impl Fn(&T) -> bool,
    ) -> Result<(String, Arc<T>), Unresolved> {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        let candidates: Vec<&Entry<T>> = entries.iter().filter(|e| accepts(&e.item)).collect();
        let hit = |e: &Entry<T>| (e.id.clone(), e.item.clone());
        if let Some(exact) = candidates.iter().find(|e| e.id == name) {
            return Ok(hit(exact));
        }
        let named: Vec<&Entry<T>> = candidates
            .iter()
            .copied()
            .filter(|e| !e.builtin() && e.local() == name)
            .collect();
        match named.as_slice() {
            [] => Err(Unresolved::Unknown(
                candidates.iter().map(|e| e.id.clone()).collect(),
            )),
            [only] => Ok(hit(only)),
            _ => Err(Unresolved::Ambiguous(
                named.iter().map(|e| e.id.clone()).collect(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Roster<str> {
        Roster::builtin([("vanilla".to_string(), Arc::from("core"))])
    }

    fn add(roster: &Roster<str>, owner: &str, local: &str) {
        let id = format!("{owner}.{local}");
        roster
            .register(owner, &id, Arc::from(id.as_str()))
            .expect("registers");
    }

    fn found(roster: &Roster<str>, name: &str) -> Option<String> {
        roster.resolve(name, |_| true).ok().map(|(id, _)| id)
    }

    #[test]
    fn a_plugin_is_registered_under_its_own_namespace_only() {
        let roster = roster();
        for refused in ["vanilla", "other.x", "acme.", "acme"] {
            assert!(matches!(
                roster.register("acme", refused, Arc::from("x")),
                Err(RegisterError::NotNamespaced { .. })
            ));
        }
        assert!(matches!(
            roster.register(BUILTIN_OWNER, "builtin.x", Arc::from("x")),
            Err(RegisterError::ReservedOwner(_))
        ));
        add(&roster, "acme", "x");
        assert!(matches!(
            roster.register("acme", "acme.x", Arc::from("y")),
            Err(RegisterError::Duplicate(_))
        ));
        assert_eq!(roster.unregister(BUILTIN_OWNER), 0);
        assert_eq!(roster.unregister("acme"), 1);
        assert_eq!(roster.snapshot().len(), 1);
    }

    #[test]
    fn a_bare_name_is_the_built_in_or_the_only_provider_and_never_a_guess() {
        let roster = roster();
        add(&roster, "acme", "vanilla");
        add(&roster, "acme", "solo");
        add(&roster, "acme", "tool");
        add(&roster, "beta", "tool");

        assert_eq!(found(&roster, "vanilla").as_deref(), Some("vanilla"));
        assert_eq!(found(&roster, "solo").as_deref(), Some("acme.solo"));
        assert_eq!(found(&roster, "acme.tool").as_deref(), Some("acme.tool"));
        assert_eq!(found(&roster, "nope"), None);
        match roster.resolve("tool", |_| true) {
            Err(Unresolved::Ambiguous(ids)) => assert_eq!(ids, ["acme.tool", "beta.tool"]),
            other => panic!("expected ambiguity, got {:?}", other.map(|(id, _)| id)),
        }

        roster.unregister("beta");
        assert_eq!(found(&roster, "tool").as_deref(), Some("acme.tool"));
    }
}
