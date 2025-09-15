use eframe::egui::{self, Color32};
use rfd::FileDialog;

use crate::{
    model::{AppState, Image},
    ui::{component::CustomSlider, ImageViewer},
};

pub struct ViewerApp {
    state: AppState,
    viewer: ImageViewer,
}

impl ViewerApp {
    pub fn new() -> Self {
        Self {
            state: AppState::empty(),
            viewer: ImageViewer::new(),
        }
    }
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // ctx.send_viewport_cmd(egui::ViewportCommand::Title("Test".to_owned()));

        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            let cur_full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!cur_full));
        }

        // Handle drag & drop events (files) at the start of frame
        // Show a visual hint while hovering
        if !ctx.input(|i| i.raw.hovered_files.is_empty()) {
            egui::Area::new(egui::Id::new("file_hover_overlay"))
                .fixed_pos(egui::pos2(16.0, 16.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label("Release to open image...");
                        for f in ctx.input(|i| i.raw.hovered_files.clone()) {
                            if let Some(path) = f.path {
                                ui.label(format!("{}", path.display()));
                            }
                        }
                    });
                });
        }
        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped_files.is_empty() {
            // Load first valid file
            for f in dropped_files {
                if let Some(path) = f.path {
                    match self.state.load_from_path(path.clone()) {
                        Ok(_) => {
                            break;
                        }
                        Err(e) => {
                            eprintln!("Failed to load dropped file: {e}");
                        }
                    }
                }
            }
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open...").clicked() {
                        ui.close();
                        if let Some(path) = FileDialog::new()
                            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "hdr", "exr"])
                            .pick_file()
                        {
                            match self.state.load_from_path(path.clone()) {
                                Ok(_) => self.viewer.reset_view(),
                                Err(e) => eprintln!("Failed to open file: {e}"),
                            }
                        }
                    }
                    if ui.button("Exit").clicked() {
                        ui.close();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.separator();
                if ui.button("Reset View").clicked() {
                    self.viewer.reset_view();
                }
                if let Some(p) = &self.state.path {
                    ui.label(format!("{}", p.display()));
                } else {
                    ui.label("(no image)");
                }
            });
        });

        egui::SidePanel::right("right").show(ctx, |ui| {
            ui.style_mut().spacing.slider_rail_height = 4.0;

            let mut display_profile_slider = |value: &mut f32, min: f32, max: f32, text: &str| {
                ui.add(
                    CustomSlider::new(value, min..=max)
                        .text(text)
                        .step_by(0.01)
                        .smart_aim(false)
                        .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 })
                        .trailing_fill(true)
                        .trailing_start(0.0)
                        .trailing_color_pos(Color32::from_hex("#4EADE4").unwrap())
                        .trailing_color_neg(Color32::from_hex("#FF6B6B").unwrap()),
                );
            };

            display_profile_slider(&mut self.state.shader_params.offset, -5.0, 5.0, "Offset");
            display_profile_slider(&mut self.state.shader_params.exposure, -5.0, 5.0, "Exposure");
            display_profile_slider(&mut self.state.shader_params.gamma, 0.1, 5.0, "Gamma");
        });

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(d) = &self.state.display {
                    ui.label(format!("Image size: {}x{}", d.spec().width, d.spec().height));
                } else {
                    ui.label("Image size: -");
                }
                ui.label(format!("Zoom: {:.2}x", self.viewer.zoom()));
            });
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(0))
            .show(ctx, |ui| {
                if let Some(d) = &self.state.display {
                    self.viewer.show_image(ui, frame, d, self.state.shader_params.clone());
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Drag & Drop an image file here.");
                    });
                }
            });
    }
}
