use std::{
    collections::{HashMap, HashSet},
    num::NonZero,
    ops::Not,
};

use anyhow::{Context, anyhow};
use derive_more::{Deref, DerefMut};
use futures::TryFutureExt;
use itertools::Itertools;
use log::trace;
use new_zealand::nz;
use rustsat::{
    encodings::{card, card::Totalizer},
    instances::{BasicVarManager, ManageVars, SatInstance},
    solvers::{Interrupt, InterruptSolver, Solve},
    types::{Assignment, constraints::CardConstraint},
};
use rustsat_glucose::simp::Glucose as GlucoseSimp;
use serde::{Deserialize, Serialize};

use crate::{
    encoder::EncodingVars,
    grid::Grid,
    platform::{PLATFORMS_DEFAULT, Platform, PlatformDef},
    point::Point,
    world::World,
};

pub mod dimensions;
pub mod encoder;
pub mod grid;
pub mod platform;
pub mod point;
mod typed_ix;
pub mod utils;
pub mod world;

const TERRAIN_SUPPORT_DISTANCE: usize = 4;

pub struct SolverConfig {
    vars: EncodingVars,
    instance: SatInstance,
}

#[derive(Debug, Clone)]
pub struct SolverRunConfig {
    pub limits: PlatformLimits,
}

impl SolverRunConfig {
    pub fn limits(&self) -> &PlatformLimits {
        &self.limits
    }

    pub fn limits_mut(&mut self) -> &mut PlatformLimits {
        &mut self.limits
    }
}

#[derive(Clone, Debug, Deref, DerefMut)]
pub struct PlatformLimits(HashMap<PlatformDef, usize>);

impl PlatformLimits {
    pub fn new(map: HashMap<PlatformDef, usize>) -> Self {
        Self(map)
    }
}

pub enum SolverResponse {
    /// A solution from a valid assignment
    Sat(Solution),
    /// No solution found for the current problem
    Unsat,
    /// The solver session was aborted by the user
    Aborted,
}

impl SolverConfig {
    pub fn new(world: &World) -> SolverConfig {
        let mut instance: SatInstance<BasicVarManager> = SatInstance::new();
        let vars = encoder::encode(&PLATFORMS_DEFAULT, world.grid(), &mut instance);

        SolverConfig { vars, instance }
    }

    pub fn vars(&self) -> &EncodingVars {
        &self.vars
    }

    pub fn instance(&self) -> &SatInstance {
        &self.instance
    }

    pub fn start(&self, cfg: &SolverRunConfig) -> anyhow::Result<(SolverFuture, Interrupter)> {
        let mut sat_solver = GlucoseSimp::default();
        let vars = self.vars.clone();
        let (cnf, mut var_manager) = self.instance.clone().into_cnf();
        sat_solver.add_cnf(cnf).context("Failed to add CNF")?;

        for (&platform_type, &limit) in cfg.limits().iter() {
            println!("Limiting {platform_type} platforms to n <= {limit}");
            let upper_constraint = if platform_type.rectangular() {
                let mut limit_vars = vec![];
                for tile_vars in vars.iter_by_points() {
                    let limit_var = var_manager.new_var();
                    limit_vars.push(limit_var);
                    for var in [platform_type.dims(), platform_type.dims().flipped()]
                        .iter()
                        .filter_map(|d| tile_vars.for_dims(*d))
                    {
                        sat_solver.add_binary(var.neg_lit(), limit_var.pos_lit())?;
                    }
                }

                CardConstraint::new_ub(limit_vars.iter().map(|var| var.pos_lit()), limit)
            } else {
                CardConstraint::new_ub(
                    vars.iter_dims_vars(platform_type.dims()).unwrap().map(|var| var.pos_lit()),
                    limit,
                )
            };

            card::encode_cardinality_constraint::<Totalizer, _>(
                upper_constraint,
                &mut sat_solver,
                &mut var_manager,
            )
            .context("failed to encode cardinality constraint")?;
        }

        let interrupter = Box::new(sat_solver.interrupter());

        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<SolverResponse> {
            use rustsat::solvers::SolverResult as SatSolverResult;
            let result = match sat_solver.solve()? {
                SatSolverResult::Sat => SolverResponse::Sat(Solution::from_assignment(
                    &sat_solver.full_solution()?,
                    &vars,
                )),
                SatSolverResult::Unsat => SolverResponse::Unsat,
                SatSolverResult::Interrupted => SolverResponse::Aborted,
            };
            Ok(result)
        });

        Ok((SolverFuture { handle }, interrupter))
    }
}

pub type Interrupter = Box<dyn InterruptSolver + Send>;

pub struct SolverFuture {
    handle: tokio::task::JoinHandle<anyhow::Result<SolverResponse>>,
}

impl SolverFuture {
    pub fn handle(&self) -> &tokio::task::JoinHandle<anyhow::Result<SolverResponse>> {
        &self.handle
    }

    pub fn future(self) -> impl Future<Output = anyhow::Result<SolverResponse>> {
        self.handle.unwrap_or_else(|join_err| Err(anyhow!(join_err)))
    }
}

// TODO maybe eventually write a proper IntoFuture
// impl IntoFuture for SolverFuture {
//     type Output = anyhow::Result<SolverResponse>;
//     type IntoFuture = _;
//
//     fn into_future(self) -> Self::IntoFuture {
//         core::future::IntoFuture::into_future(self.future())
//     }
// }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub world: World, // TODO: run configs/profiles, previous sessions, etc.
}

#[derive(Clone, Debug)]
pub struct Solution {
    platforms: HashMap<Point, Platform>,
}

impl Solution {
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

        Solution { platforms }
    }

    pub fn platforms(&self) -> &HashMap<Point, Platform> {
        &self.platforms
    }

    pub fn platform_count(&self) -> usize {
        self.platforms.len()
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
                        tile.occupied_by = Some(&plat);
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
                .filter_map(|(p, t)| t.terrain_supported.and_then(|b| b.then_some(p)))
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
