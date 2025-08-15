use std::ops::Not;

use anyhow::{Context, anyhow};
use futures::TryFutureExt;
use itertools::Itertools;
use rustsat::{
    encodings::{card, card::Totalizer},
    instances::{BasicVarManager, ManageVars, SatInstance},
    solvers::{Interrupt, InterruptSolver, Solve},
    types::constraints::CardConstraint,
};
use rustsat_glucose::simp::Glucose as GlucoseSimp;
use serde::{Deserialize, Serialize};

use crate::{
    encoder::{EncodingVars, PlatformLayout, PlatformLimits},
    platform::PLATFORMS_DEFAULT,
    world::World,
};

pub mod encoder;
pub mod grid;
pub mod math;
pub mod platform;
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

pub enum SolverResponse {
    /// A solution from a valid assignment
    Sat(PlatformLayout),
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
                SatSolverResult::Sat => SolverResponse::Sat(PlatformLayout::from_assignment(
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
