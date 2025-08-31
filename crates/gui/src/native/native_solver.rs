use anyhow::Error;
use timberborn_platform_cruncher_gui::solver_backend::{
    SessionState, SolverBackend, SolverBackendResponse, problem::Problem,
};

pub struct NativeSolver;

impl NativeSolver {
    pub fn new(rt: tokio::runtime::Runtime) -> Self {
        todo!()
    }
}

impl SolverBackend for NativeSolver {
    fn start(
        &mut self,
        problem: Problem,
        callback: Option<Box<dyn FnMut() + Send>>,
    ) -> Result<(), Error> {
        todo!()
    }

    fn session_state(&self) -> SessionState {
        todo!()
    }

    fn interrupt(&mut self) {
        todo!()
    }

    fn try_recv(&mut self) -> Option<Result<SolverBackendResponse, Error>> {
        todo!()
    }
}
