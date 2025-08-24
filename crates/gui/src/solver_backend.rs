use std::time::Duration;

#[cfg(target_arch = "wasm32")]
use batsat::callbacks::{AsyncInterrupt, AsyncInterruptHandle};
use derive_more::{Deref, DerefMut};
use futures_channel::oneshot;
use log::warn;
use rustsat::{
    instances::Cnf,
    solvers::{
        Interrupt, InterruptSolver, Solve, SolveIncremental, SolveStats, SolverResult, SolverStats,
    },
    types::{Assignment, Cl, Clause, Lit, TernaryVal, Var},
};
use timberborn_platform_cruncher::encoder::{Encoding, PlatformLimits};

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

pub struct SolverSession<S>
where
    S: Interrupt,
{
    encoding: Encoding,
    limits: PlatformLimits,
    rx: oneshot::Receiver<(anyhow::Result<SolverResult>, S)>,
    interrupter: S::Interrupter,
}

impl<S> SolverSession<S>
where
    S: Interrupt,
{
    pub fn try_recv(maybe_self: &mut Option<Self>) -> Option<SolverResponse<S>> {
        // Both empty and closed is okay
        // Closed also implies the value has already been received
        let session = maybe_self.as_mut()?;
        match session.rx.try_recv().transpose()? {
            Ok((result, solver)) => {
                let SolverSession { encoding, limits, .. } = maybe_self.take().unwrap();
                Some(SolverResponse { result, solver, encoding, limits })
            }
            Err(_) => {
                // Not running or already done
                warn!(target: "solver backend", "Session channel was closed");
                *maybe_self = None;
                None
            }
        }
    }

    pub fn interrupt(&mut self) {
        self.interrupter.interrupt();
    }
}

#[derive(Debug)]
pub struct SolverResponse<S> {
    pub result: anyhow::Result<SolverResult>,
    pub solver: S,
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

    pub fn start<S>(
        &mut self,
        encoding: Encoding,
        limits: PlatformLimits,
    ) -> anyhow::Result<SolverSession<S>>
    where
        S: Solve + Interrupt + Default + Send + 'static,
    {
        let instance = encoding.with_limits(&limits);
        let (cnf, _var_manager) = instance.into_cnf();
        let mut solver = S::default();
        solver.add_cnf(cnf)?;
        let (tx, rx) = oneshot::channel();
        let interrupter = solver.interrupter();

        let session = SolverSession::<S> { encoding, limits, rx, interrupter };

        #[cfg(not(target_arch = "wasm32"))]
        {
            _ = self.rt.spawn_blocking({
                let ctx = self.egui_ctx.clone();
                move || {
                    // If sending fails, the backend has dropped the receiver
                    _ = tx.send((solver.solve(), solver));
                    if let Some(ctx) = ctx {
                        ctx.request_repaint();
                    }
                }
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            wasm_bindgen_futures::spawn_local({
                let ctx = self.egui_ctx.clone();
                async move || {
                    // If sending fails, the backend has dropped the receiver
                    _ = tx.send((solver.solve(), solver));
                    if let Some(ctx) = ctx {
                        ctx.request_repaint();
                    }
                }
            }());
        }

        Ok(session)
    }
}

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
        self.solve_assumps(assumps)
    }

    fn core(&mut self) -> anyhow::Result<Vec<Lit>> {
        self.core()
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
