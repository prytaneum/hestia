//! The flavor catalogue as a front-end sees it: what each distribution is, what
//! it takes, and what it needs on this machine that is not here.

use proto::minecraft::{Edition, EditionCatalogue, Flavor};

use super::Engine;
use crate::edition;
use crate::minecraft::{unmet, Side};

impl Engine {
    pub async fn flavors(&self, side: Side, edition: Edition) -> Vec<Flavor> {
        let mut flavors = self.minecraft.flavors(side, edition);
        for flavor in &mut flavors {
            flavor.requires = unmet(self.minecraft.requires(side, edition, &flavor.id)).await;
        }
        flavors
    }

    /// Every edition the side offers, each with its providers and its rules.
    pub async fn catalogue(&self, side: Side) -> Vec<EditionCatalogue> {
        let mut editions = Vec::new();
        for &edition in edition::offered(side) {
            editions.push(EditionCatalogue {
                edition,
                rules: match side {
                    Side::Server => edition::config_rules(edition),
                    Side::Client => crate::instances::Instances::config_rules(),
                },
                flavors: self.flavors(side, edition).await,
            });
        }
        editions
    }
}
