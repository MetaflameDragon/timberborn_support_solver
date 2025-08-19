use std::{cmp::Ordering, collections::BTreeMap};

use eframe::epaint::Color32;
use egui::{RichText, TextEdit, Ui};
use log::info;
use timberborn_platform_cruncher::{math::Dimensions, platform::PlatformDef};

use crate::app;

pub struct PlatformTypeSelector {
    platform_defs: BTreeMap<PlatformDefOrdered, PlatformDefItemData>,
    new_platform_str: String,
    focus_text_next_frame: bool,
    text_box_feedback: Option<EntryFeedback>,
}

#[derive(Clone, Debug)]
enum EntryFeedback {
    ParseError,
    Duplicate(PlatformDef),
}

impl EntryFeedback {
    pub fn as_color(&self) -> Color32 {
        match self {
            EntryFeedback::ParseError => Color32::RED,
            EntryFeedback::Duplicate(_) => Color32::ORANGE,
        }
    }

    pub fn is_duplicate(&self, def: PlatformDef) -> bool {
        if let EntryFeedback::Duplicate(duplicate) = self { *duplicate == def } else { false }
    }
}

#[derive(Copy, Clone, Debug)]
struct PlatformDefOrdered(pub PlatformDef);

#[derive(Clone, Debug)]
struct PlatformDefItemData {
    active: bool,
}

impl PartialEq for PlatformDefOrdered {
    fn eq(&self, other: &Self) -> bool {
        self.0.dims().eq(&other.0.dims())
    }
}

impl Eq for PlatformDefOrdered {}

impl Ord for PlatformDefOrdered {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_dims = self.0.dims();
        let other_dims = other.0.dims();
        match (self_dims.width.cmp(&other_dims.width), self_dims.height.cmp(&other_dims.height)) {
            (Ordering::Equal, height_cmp) => height_cmp,
            (width_cmp, _) => width_cmp,
        }
    }
}

impl PartialOrd for PlatformDefOrdered {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PlatformDefItemData {
    pub fn new_active() -> Self {
        Self { active: true }
    }
}

impl PlatformTypeSelector {
    pub fn with_defaults(
        platform_defs: impl IntoIterator<Item = PlatformDef>,
    ) -> PlatformTypeSelector {
        let platform_defs_btree = platform_defs
            .into_iter()
            .map(|def| (PlatformDefOrdered(def), PlatformDefItemData::new_active()))
            .collect();
        dbg!(&platform_defs_btree);
        Self {
            platform_defs: platform_defs_btree,
            new_platform_str: String::new(),
            focus_text_next_frame: false,
            text_box_feedback: None,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            // Rows with all platforms
            // Items are removed by returning false from the closure
            self.platform_defs.retain(|def, data| {
                let dims = def.0.dims();
                // Disable removal/deactivation for 1x1 platforms,
                // as a lot of logic assumes that they're always available
                let disable_controls = dims != Dimensions::new(1, 1);
                ui.add_enabled_ui(disable_controls, |ui| {
                    ui.horizontal(|ui| {
                        let should_keep = !ui.button("X").clicked();
                        ui.checkbox(&mut data.active, egui::Atom::default());
                        let label_text =
                            RichText::new(format!("{}x{}", dims.width(), dims.height())).color(
                                // Highlight if a duplicate entry warning is being shown
                                self.text_box_feedback
                                    .as_ref()
                                    .and_then(|f| f.is_duplicate(def.0).then_some(f.as_color()))
                                    .unwrap_or(Color32::PLACEHOLDER),
                            );
                        ui.label(label_text);

                        should_keep
                    })
                    .inner
                })
                .inner
            });

            // Last row with an entry box
            ui.horizontal(|ui| {
                let plus_clicked = ui.button("+").clicked();

                let text_edit = TextEdit::singleline(&mut self.new_platform_str)
                    .text_color_opt(self.text_box_feedback.as_ref().map(|x| x.as_color()))
                    .show(ui);

                if self.focus_text_next_frame {
                    self.focus_text_next_frame = false;
                    text_edit.response.request_focus();
                }

                if text_edit.response.lost_focus() || text_edit.response.changed() {
                    self.text_box_feedback = None;
                }

                if plus_clicked
                    || text_edit.response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    if let Some(def) = app::try_parse_platform_def(&self.new_platform_str) {
                        if self
                            .platform_defs
                            .insert(PlatformDefOrdered(def), PlatformDefItemData::new_active())
                            .is_none()
                        {
                            dbg!(&self.platform_defs);
                            self.new_platform_str.clear();
                        } else {
                            self.text_box_feedback = Some(EntryFeedback::Duplicate(def));
                        }
                    } else {
                        info!("Failed to parse platform definition");
                        self.text_box_feedback = Some(EntryFeedback::ParseError);
                    }

                    self.focus_text_next_frame = true;
                }
            });
        });
    }

    pub fn active_platform_defs(&self) -> impl Iterator<Item = PlatformDef> {
        self.platform_defs
            .iter()
            .filter_map(|(def, &PlatformDefItemData { active, .. })| active.then_some(def.0))
    }
}
