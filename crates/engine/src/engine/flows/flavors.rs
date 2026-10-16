//! The flavor catalogue as a front-end sees it: what each distribution is, what
//! it takes, and what it needs on this machine that is not here.

use proto::minecraft::Flavor;

use super::Engine;
use crate::minecraft::{unmet, Side};

impl Engine {
    pub async fn flavors(&self, side: Side) -> Vec<Flavor> {
        let mut flavors = self.minecraft.flavors(side);
        for flavor in &mut flavors {
            flavor.requires = unmet(self.minecraft.requires(side, &flavor.id)).await;
        }
        flavors
    }
}
