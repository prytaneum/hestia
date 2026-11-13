//! The runtime-mutable provider list shared by the content and Minecraft
//! aggregates. Every provider has an owner: the built-ins are `builtin` and are
//! never removed, a plugin's are its plugin id and leave with it. An official
//! plugin's provider may take a built-in's id, and the built-in is then the
//! fallback: it answers again the moment the plugin's provider leaves or
//! stops being live.

use std::sync::{Arc, RwLock};

pub(crate) const BUILTIN_OWNER: &str = "builtin";

/// Whether a registered provider can still answer. A provider that shadows a
/// built-in is passed over while this is false, so a plugin disabled after
/// repeated traps hands its id back to the built-in without being unregistered.
pub type Live = Arc<dyn Fn() -> bool + Send + Sync>;

/// A [`Live`] that never lapses.
pub fn always_live() -> Live {
    Arc::new(|| true)
}

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
    live: Live,
}

impl<T: ?Sized> Entry<T> {
    fn builtin(&self) -> bool {
        self.owner == BUILTIN_OWNER
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
                live: always_live(),
            })
            .collect();
        Roster {
            entries: RwLock::new(entries),
        }
    }

    /// `id` must be `<owner>.<local>`, so a plugin's provider cannot take a
    /// built-in's id. An `official` owner
    /// ([0099](../../../docs/decisions/0099-official-plugins-are-signed.md))
    /// may register a bare id, and one that equals a built-in's shadows it
    /// ([0116](../../../docs/decisions/0116-an-official-provider-shadows-the-builtin.md));
    /// a bare id another plugin already holds is a duplicate.
    pub(crate) fn register(
        &self,
        owner: &str,
        official: bool,
        live: Live,
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
        let mut shadows = false;
        for existing in entries.iter().filter(|e| e.id == id) {
            if !existing.builtin() || !official || namespaced {
                return Err(RegisterError::Duplicate(id.to_string()));
            }
            shadows = true;
        }
        if shadows {
            tracing::info!(plugin = owner, provider = %id, "official provider shadows the built-in");
        }
        entries.push(Entry {
            owner: owner.to_string(),
            id: id.to_string(),
            item,
            live,
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
        let (dropped, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut *entries)
            .into_iter()
            .partition(|e| e.owner == owner);
        *entries = kept;
        for entry in &dropped {
            if entries.iter().any(|e| e.id == entry.id) {
                tracing::info!(plugin = owner, provider = %entry.id, "built-in provider restored");
            }
        }
        before - entries.len()
    }

    /// A copy of the list with one provider per id, so a caller can await on a
    /// provider without holding the lock. A live shadower stands where the
    /// built-in it shadows was listed.
    pub(crate) fn snapshot(&self) -> Vec<Arc<T>> {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        entries
            .iter()
            .filter_map(|entry| {
                let rival =
                    |other: &&Entry<T>| other.id == entry.id && other.builtin() != entry.builtin();
                if entry.builtin() {
                    let shadower = entries.iter().filter(rival).find(|e| (e.live)());
                    Some(shadower.unwrap_or(entry).item.clone())
                } else if entries.iter().any(|e| rival(&e)) {
                    None
                } else {
                    Some(entry.item.clone())
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    fn roster() -> Roster<str> {
        Roster::builtin([("modrinth".to_string(), Arc::from("modrinth"))])
    }

    fn listed(roster: &Roster<str>) -> Vec<String> {
        roster.snapshot().iter().map(|s| s.to_string()).collect()
    }

    fn add(
        roster: &Roster<str>,
        owner: &str,
        official: bool,
        id: &str,
        item: &str,
    ) -> Result<(), RegisterError> {
        roster.register(owner, official, always_live(), id, Arc::from(item))
    }

    #[test]
    fn a_plugin_entry_is_listed_until_its_owner_unregisters() {
        let roster = roster();
        add(&roster, "acme", false, "acme.hangar", "acme.hangar").expect("namespaced");
        assert_eq!(listed(&roster), ["modrinth", "acme.hangar"]);

        assert_eq!(roster.unregister("acme"), 1);
        assert_eq!(listed(&roster), ["modrinth"]);
    }

    #[test]
    fn a_builtin_can_be_neither_removed_nor_shadowed_by_a_plugin_that_is_not_official() {
        let roster = roster();
        assert_eq!(roster.unregister(BUILTIN_OWNER), 0);
        assert!(matches!(
            add(&roster, "acme", false, "modrinth", "evil"),
            Err(RegisterError::NotNamespaced { .. })
        ));
        assert!(matches!(
            add(&roster, BUILTIN_OWNER, true, "builtin.x", "evil"),
            Err(RegisterError::ReservedOwner(_))
        ));
        add(&roster, "acme", false, "acme.x", "one").expect("first");
        assert!(matches!(
            add(&roster, "acme", false, "acme.x", "two"),
            Err(RegisterError::Duplicate(_))
        ));
        assert_eq!(listed(&roster), ["modrinth", "one"]);
    }

    #[test]
    fn only_an_official_owner_registers_a_bare_id() {
        let roster = roster();
        assert!(matches!(
            add(&roster, "acme", false, "hangar", "one"),
            Err(RegisterError::NotNamespaced { .. })
        ));
        add(&roster, "acme", true, "hangar", "two").expect("official");
        assert_eq!(listed(&roster), ["modrinth", "two"]);
    }

    #[test]
    fn an_official_provider_shadows_the_builtin_until_it_leaves() {
        let roster = roster();
        add(&roster, "acme", true, "modrinth", "plugin").expect("shadows");
        assert_eq!(listed(&roster), ["plugin"]);

        assert_eq!(roster.unregister("acme"), 1);
        assert_eq!(listed(&roster), ["modrinth"]);
    }

    #[test]
    fn a_shadower_keeps_the_builtins_place_in_the_list() {
        let roster = Roster::builtin([
            ("a".to_string(), Arc::from("a")),
            ("b".to_string(), Arc::from("b")),
        ]);
        add(&roster, "acme", true, "a", "plugin-a").expect("shadows");
        add(&roster, "acme", true, "fresh", "fresh").expect("new id");
        assert_eq!(listed(&roster), ["plugin-a", "b", "fresh"]);
    }

    #[test]
    fn a_second_official_claiming_the_same_bare_id_is_a_duplicate() {
        let roster = roster();
        add(&roster, "acme", true, "modrinth", "first").expect("shadows");
        assert!(matches!(
            add(&roster, "other", true, "modrinth", "second"),
            Err(RegisterError::Duplicate(_))
        ));
        add(&roster, "acme", true, "hangar", "new").expect("new id");
        assert!(matches!(
            add(&roster, "other", true, "hangar", "again"),
            Err(RegisterError::Duplicate(_))
        ));
        assert_eq!(listed(&roster), ["first", "new"]);
    }

    #[test]
    fn a_namespaced_id_never_collides_with_a_builtin_even_for_an_official_owner() {
        let roster = Roster::builtin([("acme.x".to_string(), Arc::from("builtin"))]);
        assert!(matches!(
            add(&roster, "acme", true, "acme.x", "plugin"),
            Err(RegisterError::Duplicate(_))
        ));
    }

    #[test]
    fn a_shadower_that_is_no_longer_live_hands_the_id_back() {
        let roster = roster();
        let live = Arc::new(AtomicBool::new(true));
        let flag = live.clone();
        roster
            .register(
                "acme",
                true,
                Arc::new(move || flag.load(Ordering::Relaxed)),
                "modrinth",
                Arc::from("plugin"),
            )
            .expect("shadows");
        assert_eq!(listed(&roster), ["plugin"]);

        live.store(false, Ordering::Relaxed);
        assert_eq!(listed(&roster), ["modrinth"]);

        live.store(true, Ordering::Relaxed);
        assert_eq!(listed(&roster), ["plugin"]);
    }
}
