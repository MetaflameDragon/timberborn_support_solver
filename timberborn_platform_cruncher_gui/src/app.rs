use std::{collections::HashMap, ops::ControlFlow, sync::Arc};

use anyhow::{Context as _, anyhow, bail};
use eframe::{Frame, Storage, emath::pos2, epaint::StrokeKind};
use egui::{
    Button, Color32, Context, DragValue, Modal, PointerButton, Pos2, Rect, Response, RichText,
    Sense, Shape, Stroke, Ui, UiBuilder, Vec2, Widget, vec2,
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

use crate::{SolverBackend, app::frame_history::FrameHistory};

mod frame_history;

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
    displayed_layout: Option<PlatformLayout>,
    frame_history: FrameHistory,
}

struct CanvasClickQueue {}

impl<S> App<S>
where
    S: Interrupt,
{
    pub fn new(cc: &eframe::CreationContext<'_>, mut backend: SolverBackend<S>) -> Self {
        let terrain_grid = Grid::new(Dimensions::new(24, 24));
        backend.set_egui_ctx(cc.egui_ctx.clone());

        App {
            terrain_grid,
            resize_modal: Default::default(),
            backend,
            displayed_layout: None,
            frame_history: FrameHistory::default(),
        }
    }

    fn draw_terrain_grid_ui(&mut self, ui: &mut Ui) -> Response {
        ui.scope_builder(UiBuilder::new().sense(Sense::drag()), |ui| {
            egui::Frame::canvas(ui.style()).show(ui, |ui| {
                let corner_point = ui.next_widget_position();

                // Spacing between items
                let spacing = 2f32;
                let min_tile_size = 3f32;

                let total_width = ui.available_width();
                let tile_size = ((total_width + spacing) / (self.terrain_grid.dims().width as f32)
                    - spacing)
                    .max(min_tile_size);
                let get_offset = |p: Point, rel_tile_offset: Vec2| -> Vec2 {
                    ((pos2(p.x as f32, p.y as f32)) * (tile_size + spacing)
                        + rel_tile_offset * tile_size)
                        .to_vec2()
                };

                let grid_vec =
                    get_offset(self.terrain_grid.dims().corner_point_incl().unwrap(), Vec2::ONE);
                ui.set_width(grid_vec.x);
                ui.set_height(grid_vec.y);

                for (point, tile) in self.terrain_grid.enumerate() {
                    ui.painter().rect_filled(
                        Rect::from_two_pos(
                            corner_point + get_offset(point, Vec2::ZERO),
                            corner_point + get_offset(point, Vec2::ONE),
                        ),
                        0,
                        if tile.terrain { Color32::BROWN } else { Color32::WHITE },
                    );
                }

                if let Some(layout) = &self.displayed_layout {
                    for (_, plat) in layout.platforms() {
                        let Some((a, b)) = plat.area_corners() else {
                            continue;
                        };

                        let a_pos_rel = get_offset(a, vec2(0.25, 0.25));
                        let b_pos_rel = get_offset(b, vec2(0.75, 0.75));

                        let rect =
                            Rect::from_two_pos(corner_point + a_pos_rel, corner_point + b_pos_rel);
                        let fill_color = Color32::DARK_GREEN * Color32::from_white_alpha(64);
                        let border_color = Color32::DARK_GREEN * Color32::from_white_alpha(192);

                        ui.painter().rect_filled(rect, 0, fill_color);
                        ui.painter().rect_stroke(
                            rect,
                            0,
                            Stroke::new(1.5f32, border_color),
                            StrokeKind::Middle,
                        );
                    }
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
    fn update(&mut self, ctx: &Context, frame: &mut Frame) {
        self.frame_history.on_new_frame(ctx.input(|i| i.time), frame.info().cpu_usage);

        match self.try_get_current_session_results() {
            None => {}
            Some(SolverSessionResult::Unsat) => {
                info!("Unsat");
            }
            Some(SolverSessionResult::Sat { layout }) => {
                info!("Sat\n{layout:#?}");
                self.displayed_layout = Some(layout);
            }
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            self.frame_history.ui(ui);

            if ui.button("Resize grid").clicked() {
                self.resize_modal.open(self.terrain_grid.dims());
            }
            if let ControlFlow::Break(Some(new_dims)) = self.resize_modal.ui(ui)
                && !new_dims.empty()
            {
                self.terrain_grid = Grid::new(new_dims); // TODO copy old
            }
            debug_assert!(!self.terrain_grid.dims().empty());

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

        // Limit each axis to 1 minimum
        self.stored_value.width = self.stored_value.width.max(1);
        self.stored_value.height = self.stored_value.height.max(1);

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
