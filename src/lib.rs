use anyhow::{Context, anyhow};
use futures::TryFutureExt;
use rustsat::{
    instances::Cnf,
    solvers::{Interrupt, Solve, SolverResult},
};
use serde::{Deserialize, Serialize};

use crate::world::World;

pub mod encoder;
pub mod grid;
pub mod math;
pub mod platform;
mod typed_ix;
pub mod utils;
pub mod world;

const TERRAIN_SUPPORT_DISTANCE: usize = 4;

pub fn run_solver<S>(mut solver: S, cnf: Cnf) -> anyhow::Result<(SolverFuture<S>, S::Interrupter)>
where
    S: Solve + Interrupt + Send + 'static,
{
    solver.add_cnf(cnf).context("Failed to add CNF")?;
    let interrupter = solver.interrupter();

    let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<(SolverResult, S)> {
        Ok((solver.solve()?, solver))
    });

    Ok((SolverFuture { handle }, interrupter))
}

pub struct SolverFuture<S> {
    handle: tokio::task::JoinHandle<anyhow::Result<(SolverResult, S)>>,
}

impl<S> SolverFuture<S> {
    pub fn handle(&self) -> &tokio::task::JoinHandle<anyhow::Result<(SolverResult, S)>> {
        &self.handle
    }

    pub fn future(self) -> impl Future<Output = anyhow::Result<(SolverResult, S)>> {
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
