use std::collections::HashMap;

use derive_more::{Deref, DerefMut};

use crate::platform::PlatformDef;

#[derive(Clone, Debug, Deref, DerefMut)]
pub struct PlatformLimits(HashMap<PlatformDef, usize>);

impl PlatformLimits {
    pub fn new(map: HashMap<PlatformDef, usize>) -> Self {
        Self(map)
    }
}
