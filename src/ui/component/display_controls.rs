use eframe::egui::{self, Color32};

use crate::{
    model::MatImage,
    res::icons::Icons,
    switch,
    ui::{
        component::{
            egui_ext::{Size, UiExt},
            CustomSlider,
        },
        gl::ScaleMode,
    },
};

pub fn display_controls_ui(
    ui: &mut egui::Ui,
    icons: &Icons,
    mat_image: &MatImage,
    channel: i32,
    scale_mode: &mut ScaleMode,
    auto_minmax: &mut bool,
    min_v: &mut f32,
    max_v: &mut f32,
) {
    let ctx = ui.ctx().clone();
    let locked = *auto_minmax;

    ui.radio_icon(scale_mode, ScaleMode::Linear, icons.get_scale_linear(&ctx), "Linear");
    ui.radio_icon(scale_mode, ScaleMode::Inverse, icons.get_scale_inverse(&ctx), "Inverse");

    ui.separator();

    let original_spacing = ui.style().spacing.item_spacing;
    ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 0.0);

    let mut tmp_min = *min_v;
    let mut tmp_max = *max_v;

    if locked {
        if channel == -1 {
            tmp_min = mat_image.minmax().total_min();
            tmp_max = mat_image.minmax().total_max();
        } else {
            tmp_min = mat_image.minmax().min(channel as usize);
            tmp_max = mat_image.minmax().max(channel as usize);
        }
    }

    ui.columns_sized(
        [
            Size::remainder(1.0),
            Size::exact(24.0),
            Size::remainder(1.0),
            Size::exact(original_spacing.x),
            Size::exact(16.0),
        ],
        |columns| {
            // Min control
            columns[0]
                .add_enabled_ui(!locked, |ui| {
                    ui.add(egui::DragValue::new(switch!(locked => &mut tmp_min, min_v)).speed(0.01));
                })
                .response
                .on_hover_text("Min value");

            // Swap min/max button
            if columns[1]
                .add_enabled_ui(!locked, |ui| ui.button("↔").on_hover_text("Switch min/max"))
                .inner
                .clicked()
            {
                std::mem::swap(min_v, max_v);
            }

            // Max control
            columns[2]
                .add_enabled_ui(!locked, |ui| {
                    ui.add(egui::DragValue::new(switch!(locked => &mut tmp_max, max_v)).speed(0.01));
                })
                .response
                .on_hover_text("Max value");


            // Auto min/max toggle
            columns[4].toggle_icon(
                auto_minmax,
                icons.get_normalize(&ctx),
                "Use min / max values of image for normalization",
            );
        },
    );
}

pub fn display_profile_slider(ui: &mut egui::Ui, value: &mut f32, min: f32, max: f32, baseline: f64, text: &str) {
    ui.spacing_mut().slider_width = ui.available_width() - 128.0;
    ui.add(
        CustomSlider::new(value, min..=max)
            .text(text)
            .step_by(0.01)
            .smart_aim(false)
            .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 })
            .trailing_fill(true)
            .trailing_start(baseline)
            .trailing_color_pos(Color32::from_hex("#4EADE4").unwrap())
            .trailing_color_neg(Color32::from_hex("#FF6B6B").unwrap()),
    )
    .context_menu(|ui| {
        if ui.button("Reset").clicked() {
            *value = baseline as f32;
            ui.close();
        }
    });
}
