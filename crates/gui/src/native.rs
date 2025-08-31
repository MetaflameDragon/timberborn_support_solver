use rustsat::solvers::{Interrupt, InterruptSolver, Solve};
use rustsat_glucose::simp::Glucose as GlucoseSimp;
use timberborn_platform_cruncher_gui::app;

use crate::native::native_solver::NativeSolver;

mod native_solver;

// TODO: remake a proper backend for native

pub fn run() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;

    // Backend
    let backend = NativeSolver::new(rt);

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
