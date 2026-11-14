//! The flavor catalogue as a front-end sees it: what each distribution is, what
//! it takes, and what it needs on this machine that is not here.

use anyhow::{bail, Result};
use proto::error::ErrorInfo;
use proto::minecraft::Flavor;

use super::Engine;
use crate::minecraft::{unmet, Side};

impl Engine {
    pub async fn flavors(&self, side: Side) -> Vec<Flavor> {
        let mut flavors = self.minecraft.flavors(side);
        for flavor in &mut flavors {
            flavor.requires = unmet(&self.minecraft.requires(side, &flavor.id));
        }
        flavors
    }

    /// Refuses `flavor` when this computer lacks something it needs, before a
    /// build that would fail minutes in has started.
    pub(super) fn ensure_flavor_ready(&self, side: Side, flavor: &str) -> Result<()> {
        let id = self.minecraft.canonical(side, flavor)?;
        match unmet(&self.minecraft.requires(side, &id))
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
}
