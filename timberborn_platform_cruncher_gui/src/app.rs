use std::{collections::HashMap, ops::ControlFlow, sync::Arc};

use anyhow::{Context as _, anyhow, bail};
use eframe::{Frame, Storage};
use egui::{
    Button, Color32, Context, DragValue, Modal, PointerButton, Response, RichText, Sense, Ui,
    UiBuilder, Vec2, Widget, vec2,
};
use egui_canvas::Canvas;
use futures::{FutureExt, TryFutureExt, future::BoxFuture};
use log::{error, info, warn};
use rustsat::{
    instances::Cnf,
    solvers::{Interrupt, Solve, SolveStats, SolverResult},
};
use timberborn_platform_cruncher::{
    encoder::{Encoding, PlatformLayout, PlatformLimits},
    math::{Dimensions, Grid, Point},
    platform::PLATFORMS_DEFAULT,
    world::WorldGrid,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::SolverBackend;

#[derive(Clone, Default, Debug)]
struct TerrainTile {
    terrain: bool,
}

pub struct App<S>
where
    S: Interrupt,
{
    terrain_grid: Grid<TerrainTile>,
    resize_modal: ResizeModal,
    backend: SolverBackend<S>,
}

struct CanvasClickQueue {}

impl<S> App<S>
where
    S: Interrupt,
{
    pub fn new(cc: &eframe::CreationContext<'_>, backend: SolverBackend<S>) -> Self {
        let terrain_grid = Grid::new(Dimensions::new(24, 24));

        App { terrain_grid, resize_modal: Default::default(), backend }
    }

    fn draw_terrain_grid_ui(&mut self, ui: &mut Ui) -> Response {
        ui.scope_builder(UiBuilder::new().sense(Sense::drag()), |ui| {
            egui::Frame::canvas(ui.style()).show(ui, |ui| {
                let total_width = ui.available_width();
                ui.set_width(total_width);
                let spacing = ui.spacing_mut();
                spacing.item_spacing = vec2(2f32, 2f32);
                // Fixes sizing when the window is really small
                // Otherwise, gaps between rows can become too large
                spacing.interact_size = vec2(0f32, 0f32);

                let rect_side_len = (total_width
                    - ui.spacing().item_spacing.x * (self.terrain_grid.dims().width - 1) as f32)
                    / self.terrain_grid.dims().width as f32;

                // TODO: Rework to not use hundreds of Frame objects
                for row in self.terrain_grid.iter_rows_mut() {
                    ui.horizontal_top(|ui| {
                        ui.set_height(rect_side_len);
                        for tile in row {
                            egui::Frame::new()
                                .fill(if tile.terrain { Color32::BROWN } else { Color32::WHITE })
                                .show(ui, |ui| {
                                    ui.set_width(rect_side_len);
                                    ui.set_height(rect_side_len);
                                });
                        }
                    });
                }
            })
        })
        .response
    }

    fn try_get_current_session_results(&mut self) -> Option<SolverSessionResult>
    where
        S: Solve + SolveStats,
    {
        let resp = self.backend.try_recv()?;
        match resp.result {
            Ok(SolverResult::Sat) => match resp.solver.full_solution() {
                Ok(asgn) => {
                    let layout = PlatformLayout::from_assignment(&asgn, resp.encoding.vars());
                    Some(SolverSessionResult::Sat { layout })
                }
                Err(err) => {
                    error!("Failed to get assignment, but the solver reported SAT: {:?}", err);
                    None
                }
            },
            Ok(SolverResult::Unsat) => {
                info!("Unsat");
                Some(SolverSessionResult::Unsat)
            }
            Ok(SolverResult::Interrupted) => {
                info!("Solver interrupted");
                None
            }
            Err(err) => {
                error!("Solver error: {:?}", err);
                None
            }
        }
    }
}

#[derive(Debug)]
enum SolverSessionResult {
    Sat { layout: PlatformLayout },
    Unsat,
}

impl<S> eframe::App for App<S>
where
    S: Interrupt + Solve + SolveStats + Default + Send + 'static,
{
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        match self.try_get_current_session_results() {
            None => {}
            Some(SolverSessionResult::Unsat) => {
                info!("Unsat");
            }
            Some(SolverSessionResult::Sat { layout }) => {
                info!("Sat\n{layout:#?}");
            }
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            if ui.button("Resize grid").clicked() {
                self.resize_modal.open(self.terrain_grid.dims());
            }
            if let ControlFlow::Break(Some(new_dims)) = self.resize_modal.ui(ui) {
                self.terrain_grid = Grid::new(new_dims); // TODO copy old
            }

            let resp = self.draw_terrain_grid_ui(ui);
            if resp.dragged()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let rect = resp.rect;
                let grid_dims = self.terrain_grid.dims();

                let pos_frac = (pos - rect.left_top()) / rect.size();
                let tile_index = (
                    (pos_frac.x * grid_dims.width as f32).floor() as isize,
                    (pos_frac.y * grid_dims.height as f32).floor() as isize,
                );

                if let Some(tile) =
                    self.terrain_grid.get_mut(Point::new(tile_index.0, tile_index.1))
                {
                    if resp.dragged_by(PointerButton::Primary) {
                        tile.terrain = true;
                    } else if resp.dragged_by(PointerButton::Secondary) {
                        tile.terrain = false;
                    }
                }
                info!("{:?}", tile_index);
            }

            let solve_btn_resp =
                Button::new(RichText::new("Solve").color(Color32::GREEN).size(24f32)).ui(ui);

            if solve_btn_resp.clicked() {
                let world_grid = WorldGrid(self.terrain_grid.iter_map(|tile| tile.terrain));
                let encoding = Encoding::encode(&PLATFORMS_DEFAULT, &world_grid);
                let limits = PlatformLimits::new(HashMap::new());

                if let Err(err) = self.backend.start(encoding, limits) {
                    error!("Failed to start solver: {err}");
                }
            }
        });
    }

    fn save(&mut self, storage: &mut dyn Storage) {}
}

#[derive(Clone, Default, Debug)]
struct ResizeModal {
    modal_open: bool,
    stored_value: Dimensions,
}

impl ResizeModal {
    pub fn open(&mut self, initial_value: Dimensions) {
        self.modal_open = true;
        self.stored_value = initial_value;
    }

    pub fn ui(&mut self, ui: &mut Ui) -> ControlFlow<Option<Dimensions>> {
        if !self.modal_open {
            return ControlFlow::Break(None);
        }

        let resp = Modal::new(egui::Id::new("Resize Modal")).show(ui.ctx(), |ui| self.modal_ui(ui));

        if resp.should_close() {
            self.modal_open = false;
            return ControlFlow::Break(None);
        }

        if let ControlFlow::Break(modal_msg) = resp.inner {
            self.modal_open = false;
            return ControlFlow::Break(modal_msg);
        }
        ControlFlow::Continue(())
    }

    fn modal_ui(&mut self, ui: &mut Ui) -> ControlFlow<Option<Dimensions>> {
        ui.add(DragValue::new(&mut self.stored_value.width).speed(1).fixed_decimals(0));
        ui.add(DragValue::new(&mut self.stored_value.height).speed(1).fixed_decimals(0));
        ui.horizontal(|ui| {
            let apply_resp = ui.button("Apply");
            let cancel_resp = ui.button("Cancel");
            if apply_resp.clicked() {
                return ControlFlow::Break(Some(self.stored_value));
            }
            if cancel_resp.clicked() {
                return ControlFlow::Break(None);
            }
            ControlFlow::Continue(())
        })
        .inner
    }
}
