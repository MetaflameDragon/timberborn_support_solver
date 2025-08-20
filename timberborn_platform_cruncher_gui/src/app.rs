use std::ops::{ControlFlow, Range};

#[allow(unused_imports)] // Keeping this Anyhow import as Context would clash with egui
use anyhow::Context as _;
use eframe::Frame;
use egui::{
    Button, Color32, Context, DragValue, Modal, PointerButton, Rect, Response, RichText, Sense,
    Stroke, StrokeKind, TextStyle, Ui, UiBuilder, Vec2, Widget, pos2, util::History, vec2,
};
use itertools::Itertools;
use log::{error, info};
use platform_type_selector::PlatformTypeSelector;
use rustsat::{
    solvers::{Interrupt, Solve, SolveStats, SolverResult},
    types::{Assignment, TernaryVal},
};
use timberborn_platform_cruncher::{
    encoder,
    encoder::{Encoding, PlatformLayout, PlatformLimits},
    math::{Dimensions, Grid, Point},
    platform::{PLATFORMS_DEFAULT, PlatformDef},
    platform_def,
    world::WorldGrid,
};

use crate::{SolverBackend, SolverResponse, SolverSession, app::frame_history::FrameHistory};

mod frame_history;
mod platform_type_selector;

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
    backend: SolverBackend,
    active_session: Option<SolverSession<S>>,
    displayed_layout: Option<PlatformLayout>,
    layout_stats: PlatformLayoutStats,
    frame_history: FrameHistory,
    platform_type_selector: PlatformTypeSelector,
}

impl<S> App<S>
where
    S: Interrupt,
{
    pub fn new(cc: &eframe::CreationContext<'_>, mut backend: SolverBackend) -> Self {
        let terrain_grid = Grid::new(Dimensions::new(24, 24));
        backend.set_egui_ctx(cc.egui_ctx.clone());

        App {
            terrain_grid,
            resize_modal: Default::default(),
            backend,
            active_session: None,
            displayed_layout: None,
            frame_history: FrameHistory::default(),
            layout_stats: PlatformLayoutStats::new(5..100, 5.0),
            platform_type_selector: PlatformTypeSelector::with_defaults(PLATFORMS_DEFAULT.to_vec()),
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

    fn try_get_current_session_results(&mut self) -> Option<SolverSessionResult<S>>
    where
        S: Solve + SolveStats,
    {
        let resp = SolverSession::try_recv(&mut self.active_session)?;
        match resp.result {
            Ok(SolverResult::Sat) => match resp.solver.full_solution() {
                Ok(asgn) => {
                    let layout = PlatformLayout::from_assignment(&asgn, resp.encoding.vars());
                    Some(SolverSessionResult::Sat { layout, response: resp, assignment: asgn })
                }
                Err(err) => {
                    error!("Failed to get assignment, but the solver reported SAT: {err:?}");
                    None
                }
            },
            Ok(SolverResult::Unsat) => Some(SolverSessionResult::Unsat),
            Ok(SolverResult::Interrupted) => {
                info!("Solver interrupted");
                None
            }
            Err(err) => {
                error!("Solver error: {err:?}");
                None
            }
        }
    }
    fn start_solver(&mut self, limits: PlatformLimits)
    where
        S: Solve + Default + Send + 'static,
    {
        let world_grid = WorldGrid(self.terrain_grid.iter_map(|tile| tile.terrain));
        let encoding = Encoding::encode(
            &self.platform_type_selector.active_platform_defs().collect_vec(),
            &world_grid,
        );

        self.active_session.take().map(|mut session| session.interrupt());
        self.active_session = self
            .backend
            .start(encoding, limits)
            .map_err(|err| {
                error!("Failed to start solver: {err}");
            })
            .ok();
    }
}

#[derive(Debug)]
enum SolverSessionResult<S> {
    Sat { layout: PlatformLayout, response: SolverResponse<S>, assignment: Assignment },
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
            Some(SolverSessionResult::Sat {
                layout,
                response: SolverResponse { mut limits, encoding, .. },
                assignment,
            }) => {
                // info!("Sat\n{layout:#?}");
                info!("Sat");
                // if let Some(platform_count_limit) = layout.platform_count().checked_sub(1) {
                //     limits
                //         .card_limits
                //         .entry(platform_def!(1, 1))
                //         .insert_entry(platform_count_limit);
                //     self.start_solver(limits);
                // }

                let weight =
                    encoder::assignment_total_weight(&assignment, encoding.vars(), &limits.weights);

                info!("Got a solution with weight {weight}");
                self.layout_stats.weight.add(ctx.input(|i| i.time), weight);

                if let Some(weight_subtracted) = weight.checked_sub(1)
                    && weight_subtracted > 0
                {
                    limits.weight_limit = Some(weight_subtracted);
                    self.start_solver(limits);
                }

                self.displayed_layout = Some(layout);
            }
        };

        egui::SidePanel::left("left panel").show(ctx, |ui| {
            self.platform_type_selector.ui(ui);
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

            ui.horizontal(|ui| {
                let solve_btn_resp =
                    Button::new(RichText::new("Solve").color(Color32::GREEN).size(24f32)).ui(ui);

                if solve_btn_resp.clicked() {
                    let limits = PlatformLimits::new_with_weights(
                        Default::default(),
                        [
                            (platform_def!(1, 1), 1),
                            (platform_def!(1, 2), 1),
                            (platform_def!(1, 3), 1),
                            (platform_def!(1, 4), 1),
                            (platform_def!(1, 5), 1),
                            (platform_def!(1, 6), 1),
                            (platform_def!(3, 3), 1),
                            (platform_def!(5, 5), 1),
                        ]
                        .into_iter()
                        .collect(),
                        None,
                    );
                    self.layout_stats.clear();
                    self.start_solver(limits);
                }

                if self.active_session.is_some() {
                    ui.spinner();
                }

                if let Some(weight) = self.layout_stats.weight.latest() {
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
            });
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.active_session.take().map(|mut session| session.interrupt());
    }
}

fn try_parse_platform_def(input: &str) -> Option<PlatformDef> {
    let (a, b) = input.trim().split_once('x')?;
    let (a, b) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (a > 0 && b > 0).then_some(PlatformDef::new(Dimensions::new(a, b)))
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

#[derive(Clone, Debug)]
struct PlatformLayoutStats {
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
