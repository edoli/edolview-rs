use core::f32;
use std::path::PathBuf;

use eframe::egui::{self, Color32, Rangef, Visuals};
use rfd::FileDialog;

use crate::{
    model::{AppState, Image},
    ui::{
        component::{
            egui_ext::{ComboBoxExt, Size, UiExt},
            CustomSlider,
        },
        ImageViewer,
    },
};

pub struct ViewerApp {
    state: AppState,
    viewer: ImageViewer,
    tmp_min_v: String,
    tmp_max_v: String,
}

impl ViewerApp {
    pub fn new() -> Self {
        let state = AppState::empty();
        let shader_params = state.shader_params.clone();

        Self {
            state,
            viewer: ImageViewer::new(),

            tmp_min_v: shader_params.min_v.to_string().into(),
            tmp_max_v: shader_params.max_v.to_string().into(),
        }
    }

    #[inline]
    pub fn with_path(mut self, path: Option<PathBuf>) -> Self {
        if let Some(path) = path {
            if let Err(e) = self.state.load_from_path(path) {
                eprintln!("Failed to load image: {e}");
            }
        }
        self
    }
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        ctx.set_visuals(Visuals::dark());
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

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    if let Some(d) = &self.state.display {
                        let spec = d.spec();
                        let dtype = d.spec().dtype;

                        let cursor_color = if let Ok(pixel) = d.get_pixel_at(
                            self.state.cursor_pos.map_or(-1, |p| p.x),
                            self.state.cursor_pos.map_or(-1, |p| p.y),
                        ) {
                            pixel.iter().cloned().collect()
                        } else {
                            vec![0.0; spec.channels as usize]
                        };
                        ui.label_with_colored_rect(cursor_color, dtype);

                        let mean_color = if let Some(rect) = self.state.marquee_rect {
                            if let Ok(mean) = d.mean_value_in_rect(opencv::core::Rect {
                                x: rect.min.x as i32,
                                y: rect.min.y as i32,
                                width: (rect.max.x - rect.min.x) as i32,
                                height: (rect.max.y - rect.min.y) as i32,
                            }) {
                                mean.iter().map(|&v| v as f32).collect()
                            } else {
                                vec![0.0; d.spec().channels as usize]
                            }
                        } else {
                            vec![0.0; d.spec().channels as usize]
                        };
                        ui.label_with_colored_rect(mean_color, dtype);
                    }
                });
                if let Some(d) = &self.state.display {
                    ui.label(format!("Image size: {}x{}", d.spec().width, d.spec().height));
                } else {
                    ui.label("Image size: -");
                }
                ui.label(format!("Zoom: {:.2}x", self.viewer.zoom()));
                ui.label(" | ");
                if let Some(cursor_pos) = self.state.cursor_pos {
                    ui.label(format!("Cursor: ({}, {})", cursor_pos.x, cursor_pos.y));
                } else {
                    ui.label("Cursor: (-, -)");
                }

                ui.label(" | ");
            });
        });

        egui::SidePanel::right("right")
            .resizable(true)
            .width_range(Rangef::new(240.0, f32::INFINITY))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Right Panel");
                });
                ui.style_mut().spacing.slider_rail_height = 4.0;

                let channels = self.state.display.as_ref().map(|d| d.spec().channels).unwrap_or(0);
                let is_mono = self.state.channel_index != -1 || channels == 1;

                ui.columns_sized([Size::remainder(1.0), Size::exact(24.0), Size::remainder(1.0)], |columns| {
                    columns[0].text_edit_value(&mut self.tmp_min_v, &mut self.state.shader_params.min_v);
                    if columns[1].button("↔").on_hover_text("Switch min/max").clicked() {
                        std::mem::swap(&mut self.state.shader_params.min_v, &mut self.state.shader_params.max_v);
                        self.tmp_min_v = format!("{}", self.state.shader_params.min_v);
                        self.tmp_max_v = format!("{}", self.state.shader_params.max_v);
                    }
                    columns[2].text_edit_value(&mut self.tmp_max_v, &mut self.state.shader_params.max_v);
                });

                let mut display_profile_slider = |value: &mut f32, min: f32, max: f32, text: &str| {
                    ui.spacing_mut().slider_width = ui.available_width() - 128.0;
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

                ui.horizontal(|ui| {
                    
                    let sizes = ui.calc_sizes([Size::exact(50.0), Size::remainder(1.0)]);
                    ui.spacing_mut().combo_width = sizes[0];
                    egui::ComboBox::from_id_salt("channel_index").combo_i32(
                        ui,
                        &mut self.state.channel_index,
                        &(-1..channels).collect(),
                    );

                    ui.spacing_mut().combo_width = sizes[1];
                    if is_mono {
                        egui::ComboBox::from_id_salt("colormap_mono").combo(
                            ui,
                            &mut self.state.colormap_mono,
                            &self.state.colormap_mono_list,
                        )
                    } else {
                        egui::ComboBox::from_id_salt("colormap_rgb").combo(
                            ui,
                            &mut self.state.colormap_rgb,
                            &self.state.colormap_rgb_list,
                        )
                    };
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(0))
            .show(ctx, |ui| {
                self.viewer.show_image(ui, frame, &mut self.state);
            });
    }
}
