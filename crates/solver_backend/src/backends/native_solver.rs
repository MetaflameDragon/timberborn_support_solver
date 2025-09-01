use std::sync::{atomic, atomic::AtomicBool};

use futures_channel::{oneshot, oneshot::Canceled};
use log::{error, info, trace, warn};
use rustsat::solvers::{Solve, SolveStats, SolverResult};
use serde::{Deserialize, Serialize};
use timberborn_platform_cruncher::encoder::Encoding;

use crate::{
    SessionResult, SessionState, SolverBackend, SolverResponse,
    problem::{Problem, SessionData, Solution},
    utils::take_if_some,
};

pub struct NativeSolver<S: InterruptableSolver> {
    rt: tokio::runtime::Handle,
    session: Option<SolverSession<S>>,
}

pub trait InterruptableSolver {
    type Interrupter;

    #[must_use]
    fn get_interrupter(&mut self) -> Self::Interrupter;

    fn interrupt(interrupter: &Self::Interrupter);
}

impl<S: InterruptableSolver> NativeSolver<S> {
    pub fn new(rt: tokio::runtime::Handle) -> Self {
        Self { rt, session: None }
    }
}

impl<S> SolverBackend for NativeSolver<S>
where
    S: Default + InterruptableSolver + Solve + SolveStats + Send + 'static,
{
    type RecvError = ChannelClosed;

    fn start(
        &mut self,
        problem: Problem,
        mut callback: Option<Box<dyn FnMut() + Send>>,
    ) -> Result<(), anyhow::Error> {
        let (tx, rx) = oneshot::channel();

        let session_data = problem.session_data.clone();
        let mut solver = S::default();
        let interrupter = solver.get_interrupter();

        _ = self.rt.spawn_blocking({
            move || {
                let encoding = Encoding::encode(
                    &problem.static_data.platform_defs,
                    problem.static_data.world.grid(),
                );
                let (instance, decoder) = encoding.with_limits(&problem.session_data.limits);
                let (cnf, _var_manager) = instance.into_cnf();
                solver.add_cnf(cnf).expect("Failed to add CNF");
                let result = solver.solve();

                // If sending via tx fails, the backend has dropped the receiver

                if let Some(result) =
                    result.map_err(|err| error!("Error while solving: {:?}", err)).ok()
                {
                    if let Err(_) = match result {
                        SolverResult::Sat => {
                            info!("SAT");
                            let asgn =
                                solver.full_solution().expect("Error while getting solution");

                            let mut layout = decoder.decode_assignment(&asgn);
                            layout.run_trivial_optimization(&problem.static_data.world);

                            let solution = Solution { layout };
                            tx.send(SolverResponse::Sat(solution))
                        }
                        SolverResult::Unsat => {
                            info!("UNSAT");
                            tx.send(SolverResponse::Unsat)
                        }
                        SolverResult::Interrupted => {
                            info!("Interrupted");
                            tx.send(SolverResponse::Interrupted)
                        }
                    } {
                        error!("Error sending response: channel closed ");
                    }
                }

                if let Some(callback) = callback.as_mut() {
                    callback();
                }
            }
        });

        let session = SolverSession {
            session_data,
            rx,
            interrupter,
            was_interrupted: AtomicBool::new(false),
        };

        if let Some(prev_session) = self.session.replace(session) {
            S::interrupt(&prev_session.interrupter);
        }

        Ok(())
    }

    fn session_state(&self) -> SessionState {
        match self.session.as_ref() {
            None => SessionState::Idle,
            Some(session) => {
                if session.was_interrupted() {
                    SessionState::Interrupted
                } else {
                    SessionState::Running
                }
            }
        }
    }

    fn interrupt(&mut self) {
        if let Some(s) = self.session.as_ref() {
            s.interrupt();
        }
    }

    fn try_recv(&mut self) -> Option<Result<SessionResult, ChannelClosed>> {
        // return None if there's no session active, or if there's no response yet
        let (session, tx_result) =
            take_if_some(&mut self.session, |s| s.rx.try_recv().transpose())?;

        Some(process_tx_result(tx_result, session))
    }
}

#[derive(Copy, Clone, Debug)]
#[derive(Serialize, Deserialize)]
pub struct ChannelClosed;

fn process_tx_result<S: InterruptableSolver>(
    tx_result: Result<SolverResponse, Canceled>,
    session: SolverSession<S>,
) -> Result<SessionResult, ChannelClosed> {
    let was_interrupted = session.was_interrupted();

    // The channel might be closed if the solver was interrupted
    // It should respond with Interrupted, so this is always reported as a warning

    let response = tx_result
        .inspect_err(|_| warn!("Session channel closed unexpectedly"))
        .map_err(|_| ChannelClosed)?;

    let session_data = session.session_data;
    trace!("Received response: {:?}", response);

    // The solver might still return a response even if it has been interrupted
    // Currently, this response is simply dropped
    if was_interrupted {
        trace!("Solver was interrupted previously - dropping response");
        return Ok(SessionResult { response: SolverResponse::Interrupted, session_data });
    }

    Ok(SessionResult { response, session_data })
}

pub struct SolverSession<S: InterruptableSolver> {
    session_data: SessionData,
    rx: oneshot::Receiver<SolverResponse>,
    interrupter: S::Interrupter,
    was_interrupted: AtomicBool,
}

impl<S: InterruptableSolver> SolverSession<S> {
    pub fn was_interrupted(&self) -> bool {
        self.was_interrupted.load(atomic::Ordering::SeqCst)
    }
}

impl<S: InterruptableSolver> SolverSession<S> {
    pub fn interrupt(&self) {
        self.was_interrupted.store(true, atomic::Ordering::SeqCst);
        S::interrupt(&self.interrupter);
    }
}

mod rustsat_impls {
    use crate::backends::native_solver::InterruptableSolver;

    #[cfg(feature = "rustsat-glucose")]
    mod glucose {
        use rustsat::solvers::{Interrupt, InterruptSolver};

        use super::InterruptableSolver;

        impl InterruptableSolver for rustsat_glucose::simp::Glucose {
            type Interrupter = rustsat_glucose::simp::Interrupter;

            fn get_interrupter(&mut self) -> Self::Interrupter {
                self.interrupter()
            }

            fn interrupt(interrupter: &Self::Interrupter) {
                interrupter.interrupt();
            }
        }
    }
}
