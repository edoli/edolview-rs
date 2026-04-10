use eframe::egui;

use crate::{
    model::Recti,
    res::icons::Icons,
    ui::component::egui_ext::{Size, UiExt},
};

#[derive(Default)]
pub struct BookmarkWindowActions {
    pub add_current: bool,
    pub clear_all: bool,
    pub jump_to: Option<usize>,
    pub move_up: Option<usize>,
    pub move_down: Option<usize>,
    pub remove_index: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum BookmarkJumpMode {
    None,
    #[default]
    Center,
    Fit,
}

impl BookmarkJumpMode {
    fn label(self) -> &'static str {
        match self {
            Self::None => "Do Nothing",
            Self::Center => "Center",
            Self::Fit => "Fit",
        }
    }
}

pub fn show_bookmark_window(
    ctx: &egui::Context,
    icons: &Icons,
    open: &mut bool,
    bookmarks: &[Recti],
    active_bookmark_index: Option<usize>,
    jump_mode: &mut BookmarkJumpMode,
    add_shortcut_label: &str,
) -> BookmarkWindowActions {
    if !*open {
        return BookmarkWindowActions::default();
    }

    let mut actions = BookmarkWindowActions::default();

    egui::Window::new("Bookmarks")
        .default_pos(egui::pos2(1024.0, 72.0))
        .collapsible(false)
        .resizable(true)
        .default_width(320.0)
        .open(open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let add_clicked = ui
                    .button("Add Current")
                    .on_hover_text(format!("Add current selection as bookmark ({add_shortcut_label})"))
                    .clicked();
                if add_clicked {
                    actions.add_current = true;
                }

                if ui.add_enabled(!bookmarks.is_empty(), egui::Button::new("Clear All")).clicked() {
                    actions.clear_all = true;
                }
                ui.label("Jump:");
                egui::ComboBox::from_id_salt("bookmark_jump_mode")
                    .selected_text(jump_mode.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(jump_mode, BookmarkJumpMode::None, BookmarkJumpMode::None.label());
                        ui.selectable_value(jump_mode, BookmarkJumpMode::Center, BookmarkJumpMode::Center.label());
                        ui.selectable_value(jump_mode, BookmarkJumpMode::Fit, BookmarkJumpMode::Fit.label());
                    });
            });
            ui.separator();

            if bookmarks.is_empty() {
                ui.label("No bookmarks");
                return;
            }

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (index, rect) in bookmarks.iter().enumerate() {
                    let selected = active_bookmark_index == Some(index);

                    ui.scope(|ui| {
                        let mut style = (*ui.ctx().style()).clone();
                        style.spacing.button_padding = egui::vec2(2.0, 2.0);
                        ui.set_style(style);

                        ui.columns_sized(
                            [
                                Size::remainder(1.0),
                                Size::exact(16.0),
                                Size::exact(16.0),
                                Size::exact(16.0),
                            ],
                            |columns| {
                                let rect_label = format!("{}: {}", index + 1, rect);

                                columns[0].with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                                    if ui.selectable_label(selected, rect_label).clicked() {
                                        actions.jump_to = Some(index);
                                    }
                                });

                                let mut add_button =
                                    |col: usize,
                                     enabled: bool,
                                     icon_fn: fn(&Icons, &egui::Context) -> egui::Image<'static>,
                                     hover_text: &str,
                                     action: &mut Option<usize>| {
                                        if columns[col]
                                            .add_enabled(
                                                enabled,
                                                egui::ImageButton::new(
                                                    icon_fn(icons, ctx).fit_to_exact_size(egui::vec2(14.0, 14.0)),
                                                ),
                                            )
                                            .on_hover_text(hover_text)
                                            .clicked()
                                        {
                                            *action = Some(index);
                                        }
                                    };

                                let is_first = index == 0;
                                let is_last = index + 1 == bookmarks.len();

                                add_button(1, !is_first, Icons::get_arrow_up, "Move bookmark up", &mut actions.move_up);
                                add_button(
                                    2,
                                    !is_last,
                                    Icons::get_arrow_down,
                                    "Move bookmark down",
                                    &mut actions.move_down,
                                );
                                add_button(3, true, Icons::get_delete, "Delete bookmark", &mut actions.remove_index);
                            },
                        );
                    });
                }
            });
        });

    actions
}
