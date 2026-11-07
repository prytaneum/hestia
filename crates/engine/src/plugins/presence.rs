//! What a plugin's `presence` import does: set or clear that plugin's own
//! Discord card
//! ([0106](../../../../docs/decisions/0106-presence-is-a-host-service.md)).

use std::sync::Arc;

use proto::plugin::{Grant, Permission};
use proto::presence::Activity;

use crate::presence::Presence;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CardError {
    #[error("setting a status card needs the `presence:set` permission")]
    Denied,
    #[error("{0}")]
    Invalid(String),
}

/// One plugin's card. Dropping the last handle clears it, so a plugin that is
/// disabled, removed or unloaded stops showing its card.
pub(crate) struct PluginCard {
    plugin: Arc<str>,
    grant: Arc<Grant>,
    presence: Arc<Presence>,
}

impl PluginCard {
    pub(crate) fn new(plugin: Arc<str>, grant: Arc<Grant>, presence: Arc<Presence>) -> Self {
        PluginCard {
            plugin,
            grant,
            presence,
        }
    }

    fn allowed(&self) -> Result<(), CardError> {
        if self.grant.permissions.contains(&Permission::PresenceSet) {
            return Ok(());
        }
        Err(CardError::Denied)
    }

    pub(crate) fn set(&self, json: &str) -> Result<(), CardError> {
        self.allowed()?;
        let card: Activity = serde_json::from_str(json)
            .map_err(|error| CardError::Invalid(format!("not an activity: {error}")))?;
        let card = card.checked().map_err(CardError::Invalid)?;
        self.presence.set_plugin(&self.plugin, card);
        Ok(())
    }

    pub(crate) fn clear(&self) -> Result<(), CardError> {
        self.allowed()?;
        self.presence.clear_plugin(&self.plugin);
        Ok(())
    }
}

impl Drop for PluginCard {
    fn drop(&mut self) {
        self.presence.clear_plugin(&self.plugin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presence::Discord;

    struct Silent;

    impl Discord for Silent {
        fn connect(&mut self) -> anyhow::Result<()> {
            anyhow::bail!("no discord")
        }
        fn set(&mut self, _: &Activity) -> anyhow::Result<()> {
            Ok(())
        }
        fn clear(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
        fn close(&mut self) {}
    }

    fn card(permissions: &[Permission]) -> PluginCard {
        PluginCard::new(
            "rpc".into(),
            Arc::new(Grant {
                permissions: permissions.iter().copied().collect(),
                ..Grant::default()
            }),
            Arc::new(Presence::new(Box::new(Silent))),
        )
    }

    #[test]
    fn without_the_permission_both_calls_are_denied() {
        let card = card(&[Permission::ProcessSpawn]);
        assert!(matches!(card.set("{}"), Err(CardError::Denied)));
        assert!(matches!(card.clear(), Err(CardError::Denied)));
    }

    #[test]
    fn with_the_permission_a_valid_card_is_accepted() {
        let card = card(&[Permission::PresenceSet]);
        card.set(r#"{"details":"Playing","buttons":[{"label":"Join","url":"https://x.io"}]}"#)
            .unwrap();
        card.clear().unwrap();
    }

    #[test]
    fn a_malformed_or_over_limit_card_is_invalid() {
        let card = card(&[Permission::PresenceSet]);
        assert!(matches!(card.set("nope"), Err(CardError::Invalid(_))));
        let long = format!(r#"{{"details":"{}"}}"#, "a".repeat(129));
        assert!(matches!(card.set(&long), Err(CardError::Invalid(_))));
    }

    #[test]
    fn dropping_the_handle_clears_the_card() {
        let presence = Arc::new(Presence::new(Box::new(Silent)));
        let grant = Arc::new(Grant {
            permissions: [Permission::PresenceSet].into_iter().collect(),
            ..Grant::default()
        });
        let card = PluginCard::new("rpc".into(), grant.clone(), presence.clone());
        card.set(r#"{"details":"Playing"}"#).unwrap();
        assert_eq!(presence.plugin_cards(), 1);
        drop(card);
        assert_eq!(presence.plugin_cards(), 0);
    }
}
