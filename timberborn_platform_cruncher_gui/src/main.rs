use rustsat::solvers::{Interrupt, InterruptSolver, Solve};
use rustsat_glucose::simp::Glucose as GlucoseSimp;

use crate::solver_backend::SolverBackend;

mod app;
mod solver_backend;

fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;

    // Backend
    let backend = SolverBackend::new(rt);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([500.0, 600.0])
            .with_min_inner_size([300.0, 220.0]),
        ..Default::default()
    };
    eframe::run_native(
        "PlatformCruncher",
        native_options,
        Box::new(|cc| Ok(Box::new(app::App::<GlucoseSimp>::new(cc, backend)))),
    )
    .expect("Error while running frontend");

    Ok(())
}
