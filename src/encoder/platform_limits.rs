use std::collections::HashMap;

use crate::platform::PlatformDef;

#[derive(Clone, Debug, Default)]
pub struct PlatformLimits {
    /// Cardinality limits
    pub card_limits: HashMap<PlatformDef, usize>,
    /// Platform type weights for optimization
    pub weights: HashMap<PlatformDef, isize>,
    /// Limit for the sum of weights
    pub weight_limit: isize,
}

impl PlatformLimits {
    pub fn new_unweighted(limits: HashMap<PlatformDef, usize>) -> Self {
        Self::new_with_weights(limits, HashMap::new(), 0)
    }
    pub fn new_with_weights(
        card_limits: HashMap<PlatformDef, usize>,
        weights: HashMap<PlatformDef, isize>,
        weight_limit: isize,
    ) -> Self {
        Self { card_limits, weights, weight_limit }
    }
}
