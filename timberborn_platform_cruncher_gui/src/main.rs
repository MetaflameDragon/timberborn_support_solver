use log::warn;
use rustsat::solvers::{Interrupt, InterruptSolver, Solve, SolverResult};
use rustsat_glucose::simp::Glucose as GlucoseSimp;
use timberborn_platform_cruncher::encoder::{Encoding, PlatformLimits};
use tokio::sync::{oneshot, oneshot::error::TryRecvError};

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
    session: Option<Session<S>>,
    interrupter: Option<S::Interrupter>,
    egui_ctx: Option<egui::Context>,
}

struct Session<S> {
    encoding: Encoding,
    limits: PlatformLimits,
    rx: oneshot::Receiver<(anyhow::Result<SolverResult>, S)>,
}

pub struct SolverResponse<S> {
    pub result: anyhow::Result<SolverResult>,
    pub solver: S,
    pub encoding: Encoding,
    pub limits: PlatformLimits,
}

impl<S> SolverBackend<S>
where
    S: Interrupt,
{
    pub fn new(rt: tokio::runtime::Runtime) -> Self {
        Self { rt, session: None, interrupter: None, egui_ctx: None }
    }

    pub fn set_egui_ctx(&mut self, ctx: egui::Context) {
        self.egui_ctx = Some(ctx);
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

        self.session = Some(Session { encoding, limits, rx });

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
        Ok(())
    }

    pub fn interrupt(&mut self) {
        if let Some(interrupter) = self.interrupter.take() {
            interrupter.interrupt();
        } else {
            warn!(target: "solver backend", "Nothing to interrupt")
        }
    }

    pub fn try_recv(&mut self) -> Option<SolverResponse<S>> {
        // Both empty and closed is okay
        // Closed also implies the value has already been received
        let session = self.session.as_mut()?;
        match session.rx.try_recv() {
            Ok((result, solver)) => {
                let Session { encoding, limits, .. } = self.session.take().unwrap();
                Some(SolverResponse { result, solver, encoding, limits })
            }
            Err(TryRecvError::Empty) => {
                // In progress
                None
            }
            Err(TryRecvError::Closed) => {
                // Not running or already done
                warn!(target: "solver backend", "Session channel was closed");
                self.session = None;
                None
            }
        }
    }
}
