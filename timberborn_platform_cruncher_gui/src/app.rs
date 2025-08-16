use eframe::{Frame, Storage};
use egui::Context;

pub struct App {}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        App {}
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {}

    fn save(&mut self, storage: &mut dyn Storage) {}
}
