use std::{
    collections::{HashMap, HashSet},
    num::NonZero,
    ops::Not,
};

use itertools::Itertools;
use log::trace;
use new_zealand::nz;
use rustsat::types::Assignment;

use crate::{
    TERRAIN_SUPPORT_DISTANCE,
    encoder::EncodingVars,
    math::{Grid, Point},
    platform::{Platform, PlatformDef},
    world::World,
};

#[derive(Clone, Debug, Default)]
pub struct PlatformLayout {
    platforms: HashMap<Point, Platform>,
}

impl PlatformLayout {
    pub fn from_assignment(assignment: &Assignment, vars: &EncodingVars) -> Self {
        let mut platforms = HashMap::new();

        // iter() goes over all assigned literals (excl. DontCare)
        for (lit, plat) in assignment
            .iter()
            .filter_map(|lit| Some((lit, lit.is_pos().then(|| vars.var_to_platform(lit.var()))??)))
        {
            platforms
                .entry(plat.point())
                .and_modify(|previous: &mut Platform| {
                    if previous.def().dims() < plat.def().dims() {
                        // Update only if larger
                        *previous = plat;
                    }
                })
                .or_insert(plat);

            trace!(
                target: "solution_lit",
                "Active literal: {}", vars.lit_readable_name(lit).unwrap_or(format!("{lit:?}"))
            );
            trace!(target: "solution_lit", "=> {plat:?}");
        }

        PlatformLayout { platforms }
    }

    pub fn platforms(&self) -> &HashMap<Point, Platform> {
        &self.platforms
    }

    pub fn platform_count(&self) -> usize {
        self.platforms.len()
    }

    pub fn platform_weight_sum(&self, weights: &HashMap<PlatformDef, isize>) -> isize {
        self.platform_stats()
            .iter()
            .map(|(&def, &n)| weights.get(&def).map_or(0, |&w| w * (n.get() as isize)))
            .sum()
    }

    /// Counts the number of occurrences of all platform types and returns them
    /// as a HashMap.
    ///
    /// Platform types with 0 occurrences do not appear in the map, as indicated
    /// by the NonZero value type.
    pub fn platform_stats(&self) -> HashMap<PlatformDef, NonZero<usize>> {
        /// Since this is used only for this one folding operation, the addition
        /// simply panics on overflow, since we assume that there are no more
        /// than usize platforms for any given type.
        fn increment(n: &mut NonZero<usize>) {
            *n = n.checked_add(1).unwrap();
        }

        self.platforms().iter().fold(HashMap::new(), |mut map, (_, &plat)| {
            map.entry(plat.def()).and_modify(increment).or_insert(nz!(1));
            map
        })
    }

    pub fn get_platform(&self, p: Point) -> Option<Platform> {
        self.platforms.get(&p).copied()
    }

    pub fn validate(&self, world: &World) -> ValidationResult {
        struct Tile<'a> {
            terrain_supported: Option<bool>,
            occupied_by: Option<&'a Platform>,
        }

        let mut overlapping_platforms: HashSet<Platform> = HashSet::new();
        let mut out_of_bounds_platforms: HashSet<Platform> = HashSet::new();

        let mut tracking_grid = Grid::try_from_vec(
            world.grid().dims(),
            world
                .grid()
                .iter()
                .map(|b| Tile { terrain_supported: b.then_some(false), occupied_by: None })
                .collect_vec(),
        )
        .unwrap();

        for (_, plat) in self.platforms.iter() {
            for offset in plat.dims().iter_within() {
                let point = offset + plat.point();

                if let Some(tile) = tracking_grid.get_mut(point) {
                    if let Some(other) = tile.occupied_by {
                        // Platform overlap!
                        overlapping_platforms.insert(*plat);
                        overlapping_platforms.insert(*other);
                    } else {
                        tile.occupied_by = Some(plat);
                    }
                    // Only terrain can be supported
                    if let Some(supported) = tile.terrain_supported.as_mut() {
                        *supported = true;
                    }
                } else {
                    out_of_bounds_platforms.insert(*plat);
                }
            }
        }

        // Extend terrain support
        for _ in 0..(TERRAIN_SUPPORT_DISTANCE - 1) {
            let supported_set: HashSet<Point> = tracking_grid
                .enumerate()
                .filter_map(|(p, t)| (Some(true) == t.terrain_supported).then_some(p))
                .flat_map(Point::neighbors)
                .collect();
            for p in supported_set {
                if let Some(tile) = tracking_grid.get_mut(p)
                    && tile.terrain_supported.is_some()
                {
                    // Extend support only if there's terrain
                    tile.terrain_supported = Some(true);
                }
            }
        }

        let unsupported_terrain = tracking_grid
            .enumerate()
            .filter_map(|(p, t)| (t.terrain_supported == Some(false)).then_some(p))
            .collect();

        ValidationResult { overlapping_platforms, unsupported_terrain, out_of_bounds_platforms }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ValidationResult {
    pub unsupported_terrain: HashSet<Point>,
    pub overlapping_platforms: HashSet<Platform>,
    pub out_of_bounds_platforms: HashSet<Platform>,
}

#[derive(Clone, Debug)]
pub struct ValidationErrorPrintout {
    pub header: String,
    pub items: Vec<String>,
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        self.unsupported_terrain.is_empty()
            && self.overlapping_platforms.is_empty()
            && self.out_of_bounds_platforms.is_empty()
    }

    pub fn iter_error_printouts(&self) -> impl Iterator<Item = ValidationErrorPrintout> {
        fn format_platform(plat: &Platform) -> String {
            format!(
                "{}x{} at ({:>3};{:>3})",
                plat.dims().width,
                plat.dims().height,
                plat.point().x,
                plat.point().y
            )
        }

        [
            self.unsupported_terrain.is_empty().not().then_some(ValidationErrorPrintout {
                header: "unsupported terrain".to_string(),
                items: self
                    .unsupported_terrain
                    .iter()
                    .map(|point| format!("({:>3};{:>3})", point.x, point.y))
                    .collect(),
            }),
            self.overlapping_platforms.is_empty().not().then_some(ValidationErrorPrintout {
                header: "overlapping platforms".to_string(),
                items: self.overlapping_platforms.iter().map(format_platform).collect(),
            }),
            self.out_of_bounds_platforms.is_empty().not().then_some(ValidationErrorPrintout {
                header: "out-of-bounds platforms".to_string(),
                items: self.out_of_bounds_platforms.iter().map(format_platform).collect(),
            }),
        ]
        .into_iter()
        .flatten()
    }
}
