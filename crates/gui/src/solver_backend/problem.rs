use serde::{Deserialize, Serialize};
use timberborn_platform_cruncher::{
    encoder::{PlatformLayout, PlatformLimits},
    platform::PlatformDef,
    world::World,
};

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct Problem {
    pub static_data: StaticData,
    pub session_data: SessionData,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct StaticData {
    pub platform_defs: Vec<PlatformDef>,
    pub world: World,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct SessionData {
    pub limits: PlatformLimits,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct Solution {
    pub layout: PlatformLayout,
}
