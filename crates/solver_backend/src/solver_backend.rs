use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::problem::{Problem, SessionData, Solution};

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct SessionResult {
    pub response: SolverResponse,
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
    type RecvError;

    // TODO: better error type
    fn start(
        &mut self,
        problem: Problem,
        callback: Option<Box<dyn FnMut() + Send>>,
    ) -> Result<(), anyhow::Error>;

    fn session_state(&self) -> SessionState;

    fn interrupt(&mut self);

    fn try_recv(&mut self) -> Option<Result<SessionResult, Self::RecvError>>;
}

#[derive(Debug, Clone)]
#[derive(Serialize, Deserialize)]
pub enum SolverResponse {
    Sat(Solution),
    Unsat,
    Interrupted,
}

impl SolverResponse {
    /// Unwraps the solution into an [`Option`].
    ///
    /// Analogous to [`Result::ok`].
    pub fn solution(self) -> Option<Solution> {
        match self {
            SolverResponse::Sat(solution) => Some(solution),
            _ => None,
        }
    }
}

/// A common helper type intended for [`SolverBackend`]'s
/// [`RecvError`][`SolverBackend::RecvError`].
#[derive(Copy, Clone, Debug, Error)]
#[derive(Serialize, Deserialize)]
#[error("channel closed")]
pub struct ChannelClosed;
