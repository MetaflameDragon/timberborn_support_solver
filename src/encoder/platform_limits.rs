use std::collections::HashMap;

use crate::platform::PlatformDef;

#[derive(Clone, Debug, Default)]
pub struct PlatformLimits {
    /// Cardinality limits
    pub card_limits: HashMap<PlatformDef, usize>,
    /// Platform type weights for optimization
    pub weights: HashMap<PlatformDef, usize>,
    /// Limit for the sum of weights
    pub weight_limit: usize,
}

impl PlatformLimits {
    pub fn new_unweighted(limits: HashMap<PlatformDef, usize>) -> Self {
        Self::new_with_weights(limits, HashMap::new(), 0)
    }
    pub fn new_with_weights(
        card_limits: HashMap<PlatformDef, usize>,
        weights: HashMap<PlatformDef, usize>,
        weight_limit: usize,
    ) -> Self {
        Self { card_limits, weights, weight_limit }
    }
}
