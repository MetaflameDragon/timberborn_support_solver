use serde::{Deserialize, Serialize};

use crate::world::World;

pub mod encoder;
pub mod math;
pub mod platform;
mod typed_ix;
pub mod utils;
pub mod world;

const TERRAIN_SUPPORT_DISTANCE: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub world: World, // TODO: run configs/profiles, previous sessions, etc.
}
