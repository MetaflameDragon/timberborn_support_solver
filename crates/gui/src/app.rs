use std::ops::{ControlFlow, Range};

#[allow(unused_imports)] // Keeping this Anyhow import as Context would clash with egui
use anyhow::Context as _;
use eframe::Frame;
use egui::{
    Color32, Context, DragValue, Modal, PointerButton, Rect, Response, Sense, Stroke, StrokeKind,
    Ui, UiBuilder, Vec2, pos2, util::History, vec2,
};
use log::{error, info};
use timberborn_platform_cruncher::{
    encoder::{PlatformLayout, PlatformLimits},
    math::{Dimensions, Grid, Point},
    platform::PlatformDef,
    platform_def,
    world::{World, WorldGrid},
};

use crate::{
    app::{
        frame_history::FrameHistory, platform_type_selector::PlatformTypeSelector,
        solver_controls::SolverControlsResponse,
    },
    solver_backend::{
        SolverBackend, SolverBackendResponse,
        problem::{Problem, SessionData, StaticData},
    },
};

mod frame_history;
mod platform_type_selector;

#[derive(Clone, Default, Debug)]
struct TerrainTile {
    terrain: bool,
}

pub struct App<Backend: SolverBackend> {
    terrain_grid: Grid<TerrainTile>,
    resize_modal: ResizeModal,
    backend: Backend,
    displayed_layout: Option<PlatformLayout>,
    layout_stats: PlatformLayoutStats,
    frame_history: FrameHistory,
    platform_type_selector: PlatformTypeSelector,
}

const DEFAULT_PLATFORMS: [(PlatformDef, isize); 8] = [
    (platform_def!(1, 1), 5),
    (platform_def!(1, 2), 1),
    (platform_def!(1, 3), 1),
    (platform_def!(1, 4), 1),
    (platform_def!(1, 5), 1),
    (platform_def!(1, 6), 1),
    (platform_def!(3, 3), 2),
    (platform_def!(5, 5), 4),
];

impl<Backend: SolverBackend> App<Backend> {
    pub fn new(_cc: &eframe::CreationContext<'_>, backend: Backend) -> Self {
        let terrain_grid = Grid::new(Dimensions::new(24, 24));

        App {
            terrain_grid,
            resize_modal: Default::default(),
            backend,
            displayed_layout: None,
            frame_history: FrameHistory::default(),
            layout_stats: PlatformLayoutStats::new(5..100, 5.0),
            platform_type_selector: PlatformTypeSelector::with_defaults(DEFAULT_PLATFORMS),
        }
    }

    fn draw_terrain_grid_ui(&mut self, ui: &mut Ui) -> Response {
        ui.scope_builder(UiBuilder::new().sense(Sense::drag()), |ui| {
            egui::Frame::canvas(ui.style()).show(ui, |ui| {
                let corner_point = ui.next_widget_position();
                let available_size = ui.available_size();

                let terrain_dims = self.terrain_grid.dims();
                let terrain_dims_vec = vec2(terrain_dims.width as f32, terrain_dims.height as f32);

                // Spacing between items
                let spacing = 2f32;
                let min_tile_size = 3f32;

                let max_tile_size = (available_size + vec2(spacing, spacing)) / terrain_dims_vec
                    - vec2(spacing, spacing);
                let tile_size = max_tile_size.min_elem().max(min_tile_size);

                let get_offset = |p: Point, rel_tile_offset: Vec2| -> Vec2 {
                    ((pos2(p.x as f32, p.y as f32)) * (tile_size + spacing)
                        + rel_tile_offset * tile_size)
                        .to_vec2()
                };

                let grid_vec = get_offset(terrain_dims.corner_point_incl().unwrap(), Vec2::ONE);
                ui.set_min_size(
                    terrain_dims_vec * min_tile_size + (terrain_dims_vec - Vec2::ONE) * spacing,
                );
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
                    for plat in layout.platforms().values() {
                        let Some((a, b)) = plat.area_corners() else {
                            continue;
                        };

                        let a_pos_rel = get_offset(a, vec2(0.25, 0.25));
                        let b_pos_rel = get_offset(b, vec2(0.75, 0.75));

                        let rect =
                            Rect::from_two_pos(corner_point + a_pos_rel, corner_point + b_pos_rel);
                        let platform_color = Color32::DARK_BLUE;
                        let fill_color = platform_color * Color32::from_white_alpha(80);
                        let border_color = platform_color * Color32::from_white_alpha(212);

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

    fn try_get_current_session_results(&mut self) -> Option<SolverBackendResponse> {
        self.backend.try_recv()?.map_err(|err| error!("{:?}", err)).ok()
    }

    fn start_solver(&mut self, ctx: Context, limits: PlatformLimits) {
        let world = self.make_world();
        let platform_defs =
            self.platform_type_selector.active_platform_defs().map(|(def, _)| def).collect();

        self.backend
            .start(
                Problem {
                    static_data: StaticData { world, platform_defs },
                    session_data: SessionData { limits },
                },
                Some(Box::new(move || ctx.request_repaint())),
            )
            .map_err(|err| {
                error!("Failed to start solver: {err}");
            })
            .ok();
    }

    fn make_world(&self) -> World {
        let world_grid = WorldGrid(self.terrain_grid.iter_map(|tile| tile.terrain));
        World::new(world_grid)
    }

    fn process_active_session(&mut self, ctx: &Context) -> Option<()> {
        let results = self.try_get_current_session_results()?;

        if let Some(solution) = results.solution {
            info!("Sat");

            let mut limits = results.session_data.limits;
            let weight = solution.layout.total_weight(&limits.weights);

            info!("Got a solution with weight {weight}");
            self.layout_stats.weight.add(ctx.input(|i| i.time), weight);
            self.displayed_layout = Some(solution.layout);

            if let Some(weight_subtracted) = weight.checked_sub(1)
                && weight_subtracted > 0
            {
                limits.weight_limit = Some(weight_subtracted);
                self.start_solver(ctx.clone(), limits);
            }
        } else {
            info!("Unsat");
        }

        Some(())
    }
}

impl<Backend: SolverBackend> eframe::App for App<Backend> {
    fn update(&mut self, ctx: &Context, frame: &mut Frame) {
        self.frame_history.on_new_frame(ctx.input(|i| i.time), frame.info().cpu_usage);

        self.process_active_session(ctx);

        egui::SidePanel::left("left panel").show(ctx, |ui| {
            self.platform_type_selector.ui(ui);
        });

        egui::TopBottomPanel::bottom("bottom panel").show(ctx, |ui| {
            let session_state = self.backend.session_state();
            solver_controls::draw(ui, session_state, &self.layout_stats).map(|resp| match resp {
                SolverControlsResponse::StartSession => {
                    let platform_defs = self.platform_type_selector.active_platform_defs();
                    let limits = PlatformLimits::new_with_weights(
                        Default::default(),
                        platform_defs.collect(),
                        None,
                    );
                    self.layout_stats.clear();
                    self.start_solver(ctx.clone(), limits);
                }
                SolverControlsResponse::AbortSession => {
                    self.backend.interrupt();
                }
            });
        });

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
                info!("{tile_index:?}");
            }
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.backend.interrupt();
    }
}

fn try_parse_platform_def(input: &str) -> Option<PlatformDef> {
    let (a, b) = input.trim().split_once('x')?;
    let (a, b) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (a > 0 && b > 0).then_some(PlatformDef::new(Dimensions::new(a, b)))
}

pub mod solver_controls {
    use eframe::epaint::Color32;
    use egui::{Button, RichText, TextStyle, Ui};

    use crate::{app::PlatformLayoutStats, solver_backend::SessionState};

    #[derive(Copy, Clone, Debug)]
    pub enum SolverControlsResponse {
        StartSession,
        AbortSession,
    }

    pub fn draw(
        ui: &mut Ui,
        session_state: SessionState,
        layout_stats: &PlatformLayoutStats,
    ) -> Option<SolverControlsResponse> {
        ui.horizontal(|ui| {
            let is_running = matches!(session_state, SessionState::Running);
            let is_interrupted = matches!(session_state, SessionState::Interrupted);

            let button_text = if !is_running {
                RichText::new("Solve").color(Color32::GREEN)
            } else {
                RichText::new("Abort").color(Color32::RED)
            }
            .size(24f32);
            let solve_btn_resp = ui.add_enabled(!is_interrupted, Button::new(button_text));

            let response = solve_btn_resp.clicked().then_some(if is_running {
                SolverControlsResponse::AbortSession
            } else {
                SolverControlsResponse::StartSession
            });

            if is_running {
                ui.spinner();
            }

            if let Some(weight) = layout_stats.weight.latest() {
                ui.horizontal_wrapped(|ui| {
                    // From https://github.com/emilk/egui/blob/main/crates/egui_demo_lib/src/demo/misc_demo_window.rs#L211
                    // Trick so we don't have to add spaces in the text below:
                    let width =
                        ui.fonts(|f| f.glyph_width(&TextStyle::Body.resolve(ui.style()), ' '));
                    ui.spacing_mut().item_spacing.x = width;

                    ui.label("Solution weight: ");
                    ui.colored_label(Color32::GREEN, format!("{weight}"));
                });
            }
            response
        })
        .inner
    }
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
        ui.add(DragValue::new(&mut self.stored_value.width).speed(0.05).fixed_decimals(0));
        ui.add(DragValue::new(&mut self.stored_value.height).speed(0.05).fixed_decimals(0));

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

#[derive(Clone, Debug)]
pub struct PlatformLayoutStats {
    weight: History<isize>,
}

impl PlatformLayoutStats {
    pub fn new(length_range: Range<usize>, max_age: f32) -> Self {
        Self { weight: History::new(length_range, max_age) }
    }

    pub fn clear(&mut self) {
        self.weight.clear();
    }
}
