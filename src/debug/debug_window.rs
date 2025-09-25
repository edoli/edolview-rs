use eframe::egui::{self};
use egui_extras::{Column, TableBuilder};

use crate::debug::DEBUG_STATE;

pub fn debug_window(ctx: &egui::Context) {
    let id = egui::viewport::ViewportId::from_hash_of("edolview-debug");

    ctx.show_viewport_deferred(
        id,
        egui::viewport::ViewportBuilder::default()
            .with_title("edolview debug")
            .with_inner_size([420.0, 300.0])
            .with_resizable(true),
        move |second_ctx, _class| {
            egui::CentralPanel::default().show(second_ctx, |ui| {
                let debug_state = DEBUG_STATE.lock().unwrap();

                let available_height = ui.available_height();

                let table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto())
                    .column(Column::remainder().at_least(40.0).clip(true).resizable(true))
                    .min_scrolled_height(0.0)
                    .max_scroll_height(available_height);

                table
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.strong("Name");
                        });
                        header.col(|ui| {
                            ui.strong("Time");
                        });
                    })
                    .body(|mut body| {
                        for (_, (name, duration)) in debug_state.iter_timings().enumerate() {
                            body.row(18.0, |mut row| {
                                let timing_changed = debug_state.timing_changed(name);

                                row.col(|ui| {
                                    label_with_change(ui, name, timing_changed);
                                });
                                row.col(|ui| {
                                    label_with_change(ui, format!("{:.3?}", duration).as_str(), timing_changed);
                                });
                            });
                        }
                    });
            });

            second_ctx.request_repaint();
        },
    );
}

fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let r = (a.r() as f32 * (1.0 - t) + b.r() as f32 * t) as u8;
    let g = (a.g() as f32 * (1.0 - t) + b.g() as f32 * t) as u8;
    let b = (a.b() as f32 * (1.0 - t) + b.b() as f32 * t) as u8;
    egui::Color32::from_rgb(r, g, b)
}

pub fn label_with_change(ui: &mut egui::Ui, text: &str, changed: f32) {
    let color = ui.visuals().text_color();
    let target_color = egui::Color32::LIGHT_GREEN;
    ui.visuals_mut().override_text_color = Some(lerp_color(target_color, color, changed));
    ui.label(text);
}
