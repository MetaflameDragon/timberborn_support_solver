use std::time::Duration;

use batsat::callbacks::AsyncInterrupt;
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
use timberborn_platform_cruncher::encoder::{Encoding, PlatformLimits};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent, Worker};

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
    encoding: Encoding,
    limits: PlatformLimits,
    rx: oneshot::Receiver<Option<Assignment>>,
    interrupter: I,
}

impl<I> SolverSession<I> {
    pub fn try_recv(maybe_self: &mut Option<Self>) -> Option<SolverResponse> {
        // Both empty and closed is okay
        // Closed also implies the value has already been received
        let session = maybe_self.as_mut()?;
        match session.rx.try_recv().transpose()? {
            Ok(assignment) => {
                let SolverSession { encoding, limits, .. } = maybe_self.take().unwrap();
                Some(SolverResponse { assignment, encoding, limits })
            }
            Err(_) => {
                // Not running or already done
                warn!(target: "solver backend", "Session channel was closed");
                *maybe_self = None;
                None
            }
        }
    }
}

impl<I: InterruptSolver> SolverSession<I> {
    pub fn interrupt(&mut self) {
        self.interrupter.interrupt();
    }
}

#[derive(Debug)]
pub struct SolverResponse {
    pub assignment: Option<Assignment>,
    pub encoding: Encoding,
    pub limits: PlatformLimits,
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
        encoding: Encoding,
        limits: PlatformLimits,
    ) -> anyhow::Result<SolverSession<()>> {
        let instance = encoding.with_limits(&limits);
        let (cnf, _var_manager) = instance.into_cnf();

        let (tx, rx) = oneshot::channel();

        let session = SolverSession { encoding, limits, rx, interrupter: () };

        let worker = start_worker("./worker.js").expect("Failed to start worker");

        use wasm_bindgen::prelude::*;

        let onmessage = Closure::wrap(Box::new({
            let mut tx = Some(tx);
            move |msg: MessageEvent| {
                let response: WorkerResponse = serde_wasm_bindgen::from_value(msg.data())
                    .expect("Failed to deserialize WorkerResponse");
                web_sys::console::log_1(&format!("Got response: {response:?}").into());
                match tx.take() {
                    None => {
                        web_sys::console::warn_1(
                            &"Attempted to use SAT worker oneshot tx twice".into(),
                        );
                    }
                    Some(tx) => {
                        // Ignore error
                        _ = match response {
                            WorkerResponse::Assignment(asgn) => tx.send(Some(asgn)),

                            WorkerResponse::Unsat => tx.send(None),
                            WorkerResponse::Interrupted => tx.send(None),
                        };
                    }
                }
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget(); // The closure needs to be leaked

        let msg = serde_wasm_bindgen::to_value(&WorkerMessage::Cnf(cnf))
            .expect("Failed to serialize message");

        worker.post_message(&js_sys::Array::of1(&msg)).expect("Failed to send message to worker");

        Ok(session)
    }
}

fn start_worker(path: &str) -> Result<Worker, JsValue> {
    let worker = Worker::new(path)?;
    let msg_data = js_sys::Array::of1(&wasm_bindgen::module());
    worker.post_message(&msg_data)?;
    Ok(worker)
}

#[cfg(target_arch = "wasm32")]
pub mod batsat_wrapper {
    use std::time::Duration;

    use batsat::callbacks::{AsyncInterrupt, AsyncInterruptHandle};

    #[derive(Debug, Default)]
    pub struct BatsatWrapper(rustsat_batsat::Solver<AsyncInterrupt>);

    impl BatsatWrapper {
        pub fn batsat_ref(&self) -> &rustsat_batsat::Solver<AsyncInterrupt> {
            &self.0
        }
        pub fn batsat_mut(&mut self) -> &mut rustsat_batsat::Solver<AsyncInterrupt> {
            &mut self.0
        }
    }

    use rustsat::{
        solvers::{
            Interrupt, InterruptSolver, Solve, SolveIncremental, SolveStats, SolverResult,
            SolverStats,
        },
        types::{Cl, Clause, Lit, TernaryVal, Var},
    };

    pub struct InterruptHandleWrapper(AsyncInterruptHandle);

    impl InterruptSolver for InterruptHandleWrapper {
        fn interrupt(&self) {
            self.0.interrupt_async()
        }
    }

    impl Interrupt for BatsatWrapper {
        type Interrupter = InterruptHandleWrapper;

        fn interrupter(&mut self) -> Self::Interrupter {
            InterruptHandleWrapper(self.0.batsat_ref().cb().get_handle())
        }
    }

    // Trait forwards for Batsat

    impl Extend<Clause> for BatsatWrapper {
        fn extend<T: IntoIterator<Item = Clause>>(&mut self, iter: T) {
            self.0.extend(iter);
        }
    }

    impl<'a, C> Extend<&'a C> for BatsatWrapper
    where
        C: AsRef<Cl> + ?Sized,
    {
        fn extend<T: IntoIterator<Item = &'a C>>(&mut self, iter: T) {
            self.0.extend(iter);
        }
    }

    impl Solve for BatsatWrapper {
        fn signature(&self) -> &'static str {
            self.0.signature()
        }

        fn reserve(&mut self, max_var: Var) -> anyhow::Result<()> {
            self.0.reserve(max_var)
        }

        fn solve(&mut self) -> anyhow::Result<SolverResult> {
            self.0.solve()
        }

        fn lit_val(&self, lit: Lit) -> anyhow::Result<TernaryVal> {
            self.0.lit_val(lit)
        }

        fn add_clause_ref<C>(&mut self, clause: &C) -> anyhow::Result<()>
        where
            C: AsRef<Cl> + ?Sized,
        {
            self.0.add_clause_ref(clause)
        }
    }

    impl SolveIncremental for BatsatWrapper {
        fn solve_assumps(&mut self, assumps: &[Lit]) -> anyhow::Result<SolverResult> {
            self.0.solve_assumps(assumps)
        }

        fn core(&mut self) -> anyhow::Result<Vec<Lit>> {
            self.0.core()
        }
    }

    impl SolveStats for BatsatWrapper {
        fn stats(&self) -> SolverStats {
            self.0.stats()
        }

        fn n_sat_solves(&self) -> usize {
            self.0.n_sat_solves()
        }

        fn n_unsat_solves(&self) -> usize {
            self.0.n_unsat_solves()
        }

        fn n_terminated(&self) -> usize {
            self.0.n_terminated()
        }

        fn n_solves(&self) -> usize {
            self.0.n_solves()
        }

        fn n_clauses(&self) -> usize {
            self.0.n_clauses()
        }

        fn max_var(&self) -> Option<Var> {
            self.0.max_var()
        }

        fn n_vars(&self) -> usize {
            self.0.n_vars()
        }

        fn avg_clause_len(&self) -> f32 {
            self.0.avg_clause_len()
        }

        fn cpu_solve_time(&self) -> Duration {
            self.0.cpu_solve_time()
        }
    }
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
enum WorkerMessage {
    Cnf(Cnf),
    Interrupt,
}

#[derive(Clone, Debug)]
#[derive(Serialize, Deserialize)]
enum WorkerResponse {
    Assignment(Assignment),
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
            WorkerMessage::Cnf(cnf) => {
                self.run_solver(cnf);
            }
            WorkerMessage::Interrupt => {
                // TODO
            }
        }
    }

    fn run_solver(&mut self, cnf: Cnf) {
        web_sys::console::log_1(&JsValue::from(format!("Got CNF: {} clauses", cnf.len())));
        let mut solver = rustsat_batsat::Solver::<AsyncInterrupt>::default();
        solver.add_cnf(cnf).expect("Failed to add CNF");
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
                self.respond(&WorkerResponse::Assignment(asgn));
            }
            SolverResult::Unsat => {
                web_sys::console::log_1(&JsValue::from("UNSAT"));
                self.respond(&WorkerResponse::Unsat);
            }
            SolverResult::Interrupted => {
                web_sys::console::log_1(&JsValue::from("Interrupted"));
                self.respond(&WorkerResponse::Interrupted);
            }
        }
    }

    fn respond(&self, response: &WorkerResponse) {
        self.scope
            .post_message(
                &serde_wasm_bindgen::to_value(response).expect("Error serializing response"),
            )
            .expect("Error sending response message");
    }
}
