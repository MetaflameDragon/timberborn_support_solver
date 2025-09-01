use serde::{Deserialize, Serialize};

use crate::problem::{Problem, SessionData, Solution};

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct SolverBackendResponse {
    /// `Some` if SAT, `None` if UNSAT
    pub solution: Option<Solution>,
    /// Data and parameters for the current solution
    pub session_data: SessionData,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub enum SessionState {
    /// No session is running
    Idle,
    /// A session is active
    Running,
    /// The current session has been interrupted (but hasn't stopped yet)
    Interrupted,
}

pub trait SolverBackend {
    // TODO: better error type
    fn start(
        &mut self,
        problem: Problem,
        callback: Option<Box<dyn FnMut() + Send>>,
    ) -> Result<(), anyhow::Error>;

    fn session_state(&self) -> SessionState;

    fn interrupt(&mut self);

    // TODO: better error type
    fn try_recv(&mut self) -> Option<Result<SolverBackendResponse, anyhow::Error>>;
}

// TODO: is this necessary?
#[derive(Debug, Clone)]
pub enum SolverResponse {
    Sat(Solution),
    Unsat,
    Interrupted,
}
