use anyhow::Context;
use futures::FutureExt;
use log::{error, info, warn};
use rustsat::solvers::{Interrupt, InterruptSolver, Solve, SolveStats, SolverResult};
use rustsat_glucose::simp::Glucose as GlucoseSimp;
use timberborn_platform_cruncher::{
    encoder::{Encoding, PlatformLayout, PlatformLimits},
    *,
};
use tokio::sync::oneshot;

mod app;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;

    // Backend
    let backend = SolverBackend::<GlucoseSimp>::new(rt);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([500.0, 600.0])
            .with_min_inner_size([300.0, 220.0]),
        ..Default::default()
    };
    eframe::run_native(
        "PlatformCruncher",
        native_options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, backend)))),
    )
    .expect("Error while running frontend");

    Ok(())
}

pub struct SolverBackend<S>
where
    S: Interrupt,
{
    rt: tokio::runtime::Runtime,
    solver_rx: Option<oneshot::Receiver<(anyhow::Result<SolverResult>, S)>>,
    interrupter: Option<S::Interrupter>,
}

impl<S> SolverBackend<S>
where
    S: Interrupt,
{
    pub fn new(rt: tokio::runtime::Runtime) -> Self {
        Self { rt, solver_rx: None, interrupter: None }
    }

    pub fn start(&mut self, encoding: Encoding, limits: PlatformLimits) -> anyhow::Result<()>
    where
        S: Solve + Default + Send + 'static,
    {
        let instance = encoding.with_limits(&limits);
        let (cnf, _var_manager) = instance.into_cnf();
        let mut solver = S::default();
        solver.add_cnf(cnf)?;
        let (tx, rx) = oneshot::channel();

        self.solver_rx = Some(rx);

        _ = self.rt.spawn_blocking({
            move || {
                // If sending fails, the backend has dropped the receiver
                _ = tx.send((solver.solve(), solver));
            }
        });
        Ok(())
    }

    pub fn interrupt(&mut self) {
        if let Some(interrupter) = self.interrupter.take() {
            interrupter.interrupt();
        } else {
            warn!(target: "solver backend", "Nothing to interrupt")
        }
    }

    pub fn try_recv(&mut self) -> Option<(anyhow::Result<SolverResult>, S)> {
        // Both empty and closed is okay
        // Closed also implies the value has already been received
        self.solver_rx.as_mut()?.try_recv().ok()
    }
}
