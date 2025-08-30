use std::{
    sync::{OnceLock, atomic, atomic::AtomicBool},
    time::Duration,
};

use batsat::callbacks::{AsyncInterrupt, AsyncInterruptHandle};
use futures_channel::oneshot;
use log::warn;
use rustsat::{
    instances::Cnf,
    solvers::{
        Interrupt, InterruptSolver, Solve, SolveIncremental, SolveStats, SolverResult, SolverStats,
    },
    types::{Assignment, Cl, Clause, Lit, TernaryVal, Var},
};
use serde::{Deserialize, Serialize};
use timberborn_platform_cruncher::{
    encoder::{Decoder, Encoding, PlatformLimits},
    platform::PlatformDef,
    world::World,
};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent, Worker};

use crate::solver_backend::{
    batsat_wrapper::OnceLockInterrupter,
    problem::{Problem, SessionData, Solution},
};

pub struct SolverBackend {
    #[cfg(not(target_arch = "wasm32"))]
    rt: tokio::runtime::Runtime,
    egui_ctx: Option<egui::Context>,
}

#[cfg(not(target_arch = "wasm32"))]
pub trait SolverTraitBounds: Interrupt {}

#[cfg(not(target_arch = "wasm32"))]
impl<T> SolverTraitBounds for T where T: Interrupt {}

#[cfg(target_arch = "wasm32")]
pub trait SolverTraitBounds {}

#[cfg(target_arch = "wasm32")]
impl<T> SolverTraitBounds for T {}

pub struct SolverSession<I> {
    session_data: SessionData,
    rx: oneshot::Receiver<SolverResponse>,
    interrupter: I,
    was_interrupted: AtomicBool,
}

impl<I> SolverSession<I> {
    pub fn try_recv(maybe_self: &mut Option<Self>) -> Option<SessionResult> {
        // Both empty and closed is okay
        // Closed also implies the value has already been received
        let session = maybe_self.as_mut()?;
        match session.rx.try_recv().transpose()? {
            Ok(solver_response) => {
                let session = maybe_self.take().unwrap();
                let was_interrupted = session.was_interrupted();
                let SolverSession { session_data, .. } = session;
                Some(SessionResult { solver_response, session_data, was_interrupted })
            }
            Err(_) => {
                // Not running or already done
                warn!(target: "solver backend", "Session channel was closed");
                *maybe_self = None;
                None
            }
        }
    }

    pub fn was_interrupted(&self) -> bool {
        self.was_interrupted.load(atomic::Ordering::SeqCst)
    }
}

impl<I: InterruptSolver> SolverSession<I> {
    pub fn interrupt(&self) {
        self.was_interrupted.store(true, atomic::Ordering::SeqCst);
        self.interrupter.interrupt();
    }
}

#[derive(Debug, Clone)]
pub enum SolverResponse {
    Sat(Solution),
    Unsat,
    Interrupted,
}

#[derive(Debug, Clone)]
pub struct SessionResult {
    pub solver_response: SolverResponse,
    pub session_data: SessionData,
    pub was_interrupted: bool,
}

impl SolverBackend {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(rt: tokio::runtime::Runtime) -> Self {
        Self { rt, egui_ctx: None }
    }
    #[cfg(target_arch = "wasm32")]
    pub fn new() -> Self {
        Self { egui_ctx: None }
    }

    pub fn set_egui_ctx(&mut self, ctx: egui::Context) {
        self.egui_ctx = Some(ctx);
    }

    pub fn start(
        &mut self,
        problem: Problem,
    ) -> anyhow::Result<SolverSession<OnceLockInterrupter>> {
        let encoding =
            Encoding::encode(&problem.static_data.platform_defs, problem.static_data.world.grid());

        let (tx, rx) = oneshot::channel();

        let session = SolverSession {
            session_data: problem.session_data.clone(),
            rx,
            interrupter: OnceLockInterrupter::default(),
            was_interrupted: AtomicBool::new(false),
        };

        let worker = start_worker("./worker.js").expect("Failed to start worker");

        use wasm_bindgen::prelude::*;

        let onmessage = Closure::wrap(Box::new({
            let mut tx = Some(tx);
            let interrupter_once_lock = session.interrupter.clone();
            move |msg: MessageEvent| {
                let response: WorkerResponse = serde_wasm_bindgen::from_value(msg.data())
                    .expect("Failed to deserialize WorkerResponse");
                web_sys::console::log_1(&format!("Got response: {response:?}").into());
                let tx_msg = match response {
                    WorkerResponse::InterruptHandle { ptr } => {
                        // SAFETY: This got sent from the worker, and should be fully
                        // owned by SolverBackend
                        let handle = unsafe { Box::from_raw(ptr as *mut AsyncInterruptHandle) };
                        web_sys::console::trace_1(&"Setting interrupter".into());
                        if interrupter_once_lock.set(handle).is_err() {
                            web_sys::console::error_1(
                                &"Tried to init interrupter OnceLock twice!".into(),
                            )
                        }
                        None
                    }
                    WorkerResponse::Sat(solution) => Some(SolverResponse::Sat(solution)),
                    WorkerResponse::Unsat => Some(SolverResponse::Unsat),
                    WorkerResponse::Interrupted => Some(SolverResponse::Interrupted),
                };

                if let Some(msg) = tx_msg {
                    match tx.take() {
                        None => {
                            web_sys::console::warn_1(
                                &"Attempted to use SAT worker oneshot tx twice".into(),
                            );
                        }
                        Some(tx) => {
                            // Ignore error
                            _ = tx.send(msg);
                        }
                    }
                }
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget(); // The closure needs to be leaked

        let msg = serde_wasm_bindgen::to_value(&WorkerMessage::Problem(problem))
            .expect("Failed to serialize message");

        worker.post_message(&js_sys::Array::of1(&msg)).expect("Failed to send message to worker");

        Ok(session)
    }
}

fn start_worker(path: &str) -> Result<Worker, JsValue> {
    let worker = Worker::new(path)?;
    let msg_data = js_sys::Array::of2(&wasm_bindgen::module(), &wasm_bindgen::memory());
    worker.post_message(&msg_data)?;
    Ok(worker)
}

pub mod problem {
    use serde::{Deserialize, Serialize};
    use timberborn_platform_cruncher::{
        encoder::{PlatformLayout, PlatformLimits},
        platform::PlatformDef,
        world::World,
    };

    #[derive(Clone, Debug)]
    #[derive(Serialize, Deserialize)]
    pub struct Problem {
        pub static_data: StaticData,
        pub session_data: SessionData,
    }

    #[derive(Clone, Debug)]
    #[derive(Serialize, Deserialize)]
    pub struct StaticData {
        pub platform_defs: Vec<PlatformDef>,
        pub world: World,
    }

    #[derive(Clone, Debug)]
    #[derive(Serialize, Deserialize)]
    pub struct SessionData {
        pub limits: PlatformLimits,
    }

    #[derive(Clone, Debug)]
    #[derive(Serialize, Deserialize)]
    pub struct Solution {
        pub layout: PlatformLayout,
    }
}

#[cfg(target_arch = "wasm32")]
pub mod batsat_wrapper {
    use std::sync::{Arc, OnceLock, atomic, atomic::AtomicBool};

    use batsat::callbacks::AsyncInterruptHandle;
    use rustsat::solvers::InterruptSolver;

    #[derive(Default)]
    pub struct OnceLockInterrupter {
        handle: Arc<OnceLock<Box<AsyncInterruptHandle>>>,
        triggered: AtomicBool,
    }

    impl OnceLockInterrupter {
        pub fn set(
            &self,
            value: Box<AsyncInterruptHandle>,
        ) -> Result<(), Box<AsyncInterruptHandle>> {
            self.handle.set(value)?;

            // If already triggered, interrupt immediately
            if self.triggered.load(atomic::Ordering::Relaxed) {
                web_sys::console::trace_1(&"Interrupting late".into());
                self.handle.get().unwrap().interrupt_async();
            }
            Ok(())
        }
    }

    impl Clone for OnceLockInterrupter {
        fn clone(&self) -> Self {
            Self { handle: Arc::clone(&self.handle), triggered: AtomicBool::new(false) }
        }
    }

    impl InterruptSolver for OnceLockInterrupter {
        fn interrupt(&self) {
            web_sys::console::trace_1(&"Interrupting solver".into());
            self.triggered.store(true, atomic::Ordering::SeqCst);

            if let Some(handle) = self.handle.get() {
                handle.interrupt_async();
            } else {
                web_sys::console::trace_1(&"Interrupter not set, will be interrupted ".into());
            }
        }
    }
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
enum WorkerMessage {
    Problem(Problem),
    Interrupt,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
enum WorkerResponse {
    InterruptHandle { ptr: u32 },
    Sat(Solution),
    Unsat,
    Interrupted,
}

#[wasm_bindgen]
pub struct SatWorker {
    scope: DedicatedWorkerGlobalScope,
}

#[wasm_bindgen]
impl SatWorker {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<SatWorker, JsValue> {
        console_error_panic_hook::set_once();
        let scope = DedicatedWorkerGlobalScope::from(JsValue::from(js_sys::global()));
        Ok(SatWorker { scope })
    }

    #[wasm_bindgen]
    pub fn handle_worker_msg(&mut self, msg_ser: JsValue) -> Result<(), JsValue> {
        self.handle_msg(serde_wasm_bindgen::from_value(msg_ser)?);
        Ok(())
    }
}

impl SatWorker {
    fn handle_msg(&mut self, msg: WorkerMessage) {
        match msg {
            WorkerMessage::Problem(problem) => {
                self.run_solver(problem);
            }
            WorkerMessage::Interrupt => {
                // TODO
            }
        }
    }

    fn run_solver(&mut self, problem: Problem) {
        web_sys::console::log_1(&"Starting solver".into());
        let encoding =
            Encoding::encode(&problem.static_data.platform_defs, problem.static_data.world.grid());
        let (instance, decoder) = encoding.with_limits(&problem.session_data.limits);
        let (cnf, _var_manager) = instance.into_cnf();

        web_sys::console::log_1(&format!("Encoded CNF: {} clauses", cnf.len()).into());

        let mut solver = rustsat_batsat::Solver::<AsyncInterrupt>::default();
        solver.add_cnf(cnf).expect("Failed to add CNF");
        let handle = Box::new(solver.batsat_ref().cb().get_handle());
        let handle_ptr = Box::into_raw(handle);
        if let Err(err) = self.respond(&WorkerResponse::InterruptHandle { ptr: handle_ptr as u32 })
        {
            drop(unsafe { Box::from_raw(handle_ptr) });
            panic!("Failed to send interrupter: {err:?}")
        }

        web_sys::console::log_1(&JsValue::from("Running solver"));
        let res = match solver.solve() {
            Ok(res) => res,
            Err(err) => {
                web_sys::console::error_1(&JsValue::from(format!("Error while solving: {err:?}")));
                return;
            }
        };

        match res {
            SolverResult::Sat => {
                web_sys::console::log_1(&JsValue::from("SAT"));
                let asgn = solver.full_solution().expect("Error while getting solution");

                let mut layout = decoder.decode_assignment(&asgn);
                layout.run_trivial_optimization(&problem.static_data.world);

                let solution = Solution { layout };
                self.respond(&WorkerResponse::Sat(solution))
            }
            SolverResult::Unsat => {
                web_sys::console::log_1(&JsValue::from("UNSAT"));
                self.respond(&WorkerResponse::Unsat)
            }
            SolverResult::Interrupted => {
                web_sys::console::log_1(&JsValue::from("Interrupted"));
                self.respond(&WorkerResponse::Interrupted)
            }
        }
        .expect("Error sending response");
    }

    fn respond(&self, response: &WorkerResponse) -> Result<(), JsValue> {
        self.scope.post_message(
            &serde_wasm_bindgen::to_value(response).expect("Error serializing response"),
        )
    }
}
