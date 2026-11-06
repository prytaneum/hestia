//! The runtime-mutable provider list shared by the content and Minecraft
//! aggregates. Every provider has an owner: the built-ins are `builtin` and are
//! never removed, a plugin's are its plugin id and leave with it.

use std::sync::{Arc, RwLock};

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

struct Entry<T: ?Sized> {
    owner: String,
    id: String,
    item: Arc<T>,
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

    /// `id` must be `<owner>.<local>`, so a plugin's provider cannot shadow a
    /// built-in, whose ids carry no dot-prefixed owner. An `official` owner
    /// ([0099](../../../docs/decisions/0099-official-plugins-are-signed.md))
    /// may also register a bare id, which is how it replaces a core feature.
    pub(crate) fn register(
        &self,
        owner: &str,
        official: bool,
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
        if !namespaced && !official {
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
    /// the lock.
    pub(crate) fn snapshot(&self) -> Vec<Arc<T>> {
        self.entries
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|e| e.item.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Roster<str> {
        Roster::builtin([("modrinth".to_string(), Arc::from("modrinth"))])
    }

    fn listed(roster: &Roster<str>) -> Vec<String> {
        roster.snapshot().iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_plugin_entry_is_listed_until_its_owner_unregisters() {
        let roster = roster();
        roster
            .register("acme", false, "acme.hangar", Arc::from("acme.hangar"))
            .expect("namespaced");
        assert_eq!(listed(&roster), ["modrinth", "acme.hangar"]);

        assert_eq!(roster.unregister("acme"), 1);
        assert_eq!(listed(&roster), ["modrinth"]);
    }

    #[test]
    fn a_builtin_can_be_neither_removed_nor_shadowed() {
        let roster = roster();
        assert_eq!(roster.unregister(BUILTIN_OWNER), 0);
        assert!(matches!(
            roster.register("acme", false, "modrinth", Arc::from("evil")),
            Err(RegisterError::NotNamespaced { .. })
        ));
        assert!(matches!(
            roster.register(BUILTIN_OWNER, true, "builtin.x", Arc::from("evil")),
            Err(RegisterError::ReservedOwner(_))
        ));
        roster
            .register("acme", false, "acme.x", Arc::from("one"))
            .expect("first");
        assert!(matches!(
            roster.register("acme", false, "acme.x", Arc::from("two")),
            Err(RegisterError::Duplicate(_))
        ));
        assert_eq!(listed(&roster), ["modrinth", "one"]);
    }

    #[test]
    fn only_an_official_owner_registers_a_bare_id() {
        let roster = roster();
        assert!(matches!(
            roster.register("acme", false, "hangar", Arc::from("one")),
            Err(RegisterError::NotNamespaced { .. })
        ));
        roster
            .register("acme", true, "hangar", Arc::from("two"))
            .expect("official");
        assert_eq!(listed(&roster), ["modrinth", "two"]);
    }
}
