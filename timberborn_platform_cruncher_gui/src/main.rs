use log::{error, info};
use timberborn_platform_cruncher::{
    encoder::{Encoding, PlatformLayout, PlatformLimits},
    *,
};
use tokio::sync::{broadcast, mpsc};

mod app;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;

    // Backend
    let (backend, fut) = SolverBackendHandle::spawn();

    let backend_task_handle = rt.spawn(fut);
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

    rt.block_on(async move {
        match backend_task_handle.await {
            Ok(()) => {}
            Err(err) => {
                error!("Backend error: {err:?}")
            }
        };
    });

    Ok(())
}

pub struct SolverBackendHandle {
    tx_chan: mpsc::Sender<SolverRequest>,
    rx_chan: mpsc::Receiver<SolverResponse>,
}

impl SolverBackendHandle {
    pub fn spawn() -> (SolverBackendHandle, impl Future<Output = ()>) {
        let (req_tx, req_rx) = mpsc::channel::<SolverRequest>(100);
        let (resp_tx, resp_rx) = mpsc::channel::<SolverResponse>(100);

        let backend = SolverBackendHandle { tx_chan: req_tx, rx_chan: resp_rx };

        (backend, run_backend(req_rx, resp_tx))
    }
}

async fn run_backend(
    mut req_rx: mpsc::Receiver<SolverRequest>,
    resp_tx: mpsc::Sender<SolverResponse>,
) {
    loop {
        if let Some(SolverRequest::Stop) = req_rx.recv().await {
            info!("Stopping");
            break;
        }
    }
}

pub enum SolverRequest {
    Start { encoding: Encoding, limits: PlatformLimits },
    Interrupt,
    Stop,
}

pub enum SolverResponse {
    Layout { layout: PlatformLayout, limits: PlatformLimits },
    Unsat { limits: PlatformLimits },
    Error(anyhow::Error),
}
