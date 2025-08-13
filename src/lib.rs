use std::{
    array,
    collections::HashMap,
    io::Write,
    iter::once,
    num::NonZero,
    ops::{Add, AddAssign},
    pin::Pin,
    sync::{Arc, Mutex},
    task::Poll,
};

use anyhow::{Context, anyhow};
use derive_more::{Deref, DerefMut};
use enum_iterator::Sequence;
use enum_map::EnumMap;
use futures::{FutureExt, SinkExt, Stream, StreamExt, TryFutureExt};
use log::{info, trace};
use new_zealand::nz;
use rustsat::{
    encodings::{card, card::Totalizer},
    instances::{BasicVarManager, SatInstance},
    solvers::{Interrupt, InterruptSolver, Solve},
    types::{Assignment, Var, constraints::CardConstraint},
};
use rustsat_glucose::simp::Glucose as GlucoseSimp;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::{
    encoder::EncodingVars,
    platform::{PLATFORMS_DEFAULT, Platform, PlatformDef},
    point::Point,
    world::World,
};

pub mod dimensions;
pub mod encoder;
pub mod grid;
pub mod platform;
pub mod point;
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
    pub fn max_platforms(&self) -> Option<usize> {
        todo!()
        // self.limits.get(&PlatformDef::Square1x1).copied()
    }

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
            // TODO: Actually encode limits for other platforms
            // TODO: Clarify what the encoding means
            // The cardinality constraints still work in terms of "platform
            // promotion", where smaller platforms "promote" to
            // larger ones, and the vars for larger ones imply all
            // their predecessors. This means that limiting 5x5 to
            // <= 2 and 3x3 to <= 4 actually means "at most 4 of 3x3
            // or larger, and at most 2 of 5x5". info!("Limiting {}
            // platforms to n <= {}", platform_type, limit);
            // let upper_constraint = CardConstraint::new_ub(
            //     vars.platform_vars_map(platform_type).values().map(|var|
            // var.pos_lit()),     limit,
            // );
            //
            // card::encode_cardinality_constraint::<Totalizer, _>(
            //     upper_constraint,
            //     &mut sat_solver,
            //     &mut var_manager,
            // )
            // .context("failed to encode cardinality constraint")?;
        }

        // TODO: Temporary 1x1-only limiter
        if let (platform_type, Some(&limit)) =
            (PLATFORMS_DEFAULT[0], cfg.limits().get(&PLATFORMS_DEFAULT[0]))
        {
            println!("Limiting {} platforms to n <= {}", platform_type, limit);
            let upper_constraint = CardConstraint::new_ub(
                vars.iter_dims_vars(platform_type.dims()).unwrap().map(|var| var.pos_lit()),
                limit,
            );

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
}
