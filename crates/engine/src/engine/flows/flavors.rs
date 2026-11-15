//! The flavor catalogue as a front-end sees it: what each distribution is, what
//! it takes, and what it needs on this machine that is not here.

use anyhow::{bail, Result};
use proto::error::ErrorInfo;
use proto::minecraft::{Edition, EditionCatalogue, Flavor};

use super::Engine;
use crate::edition;
use crate::minecraft::{unmet, Side};

impl Engine {
    pub async fn flavors(&self, side: Side, edition: Edition) -> Vec<Flavor> {
        let mut flavors = self.minecraft.flavors(side, edition);
        for flavor in &mut flavors {
            flavor.requires = unmet(&self.minecraft.requires(side, edition, &flavor.id));
        }
        flavors
    }

    /// Refuses `flavor` when this computer lacks something it needs, before a
    /// build that would fail minutes in has started.
    pub(super) fn ensure_flavor_ready(
        &self,
        side: Side,
        edition: Edition,
        flavor: &str,
    ) -> Result<()> {
        let id = self.minecraft.canonical(side, edition, flavor)?;
        match unmet(&self.minecraft.requires(side, edition, &id))
            .into_iter()
            .next()
        {
            Some(requirement) => bail!(ErrorInfo::MissingRequirement {
                flavor: id,
                name: requirement.name,
                url: requirement.url,
            }),
            None => Ok(()),
        }
    }

    /// Every edition the side offers, each with its providers and its rules.
    pub async fn flavor_catalogue(&self, side: Side) -> Vec<EditionCatalogue> {
        let mut editions = Vec::new();
        for &edition in edition::offered(side) {
            editions.push(EditionCatalogue {
                edition,
                rules: match side {
                    Side::Server => edition::config_rules(edition),
                    Side::Client => crate::instances::Instances::config_rules(),
                },
                flavors: self.flavors(side, edition).await,
                name_key: (side == Side::Server).then(|| edition::name_key(edition).to_string()),
            });
        }
        editions
    }
}
