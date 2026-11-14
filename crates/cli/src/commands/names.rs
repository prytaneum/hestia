//! How a provider id is shown. The daemon always speaks full ids
//! (`<plugin>.<id>`); a person reads the provider-id alone while no other
//! registered provider of that kind shares it, which is also exactly when the
//! daemon accepts it as a bare name.

use std::collections::BTreeSet;

use anyhow::Result;
use client::proto::error::EntryKind;
use client::proto::naming::provider_id;
use client::Client;

pub struct Names(BTreeSet<String>);

impl Names {
    pub fn of(ids: impl IntoIterator<Item = String>) -> Self {
        Names(ids.into_iter().collect())
    }

    /// The flavors of one side, the scope a bare flavor name resolves in.
    pub async fn flavors(client: &Client, kind: EntryKind) -> Result<Self> {
        let flavors = client.entry().flavors(kind).await?;
        Ok(Names::of(flavors.into_iter().map(|f| f.id)))
    }

    pub async fn sources(client: &Client) -> Result<Self> {
        let sources = client.content().sources().await?;
        Ok(Names::of(sources.into_iter().map(|s| s.id)))
    }

    pub fn show(&self, id: &str) -> String {
        let short = provider_id(id);
        let shared = self
            .0
            .iter()
            .filter(|other| provider_id(other) == short)
            .count();
        match (shared, self.0.contains(id)) {
            (1, true) => short.to_string(),
            _ => id.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_provider_id_is_shown_bare_until_another_provider_shares_it() {
        let names =
            Names::of(["vanilla", "hestia.fabric", "acme.paper", "beta.paper"].map(String::from));
        assert_eq!(names.show("hestia.fabric"), "fabric");
        assert_eq!(names.show("vanilla"), "vanilla");
        assert_eq!(names.show("acme.paper"), "acme.paper");
        assert_eq!(names.show("gone.thing"), "gone.thing");
    }
}
