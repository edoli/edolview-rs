use core::f32;
use std::{cell::OnceCell, path::PathBuf};

use eframe::egui::{self, vec2, Color32, Rangef, Visuals};
use rfd::FileDialog;

use crate::{
    model::{AppState, Image, Recti},
    res,
    ui::{
        component::{
            egui_ext::{ComboBoxExt, Size, UiExt},
            CustomSlider,
        },
        gl::ScaleMode,
        icon::{self, IconExt},
        ImageViewer,
    },
    util::math_ext::vec2i,
};

const SELECT_ALL_SC: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::A);
const SELECT_NONE_SC: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Escape);
const COPY_SC: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::D);

pub struct ViewerApp {
    state: AppState,
    viewer: ImageViewer,
    last_path: Option<PathBuf>,
    tmp_min_v: String,
    tmp_max_v: String,
    tmp_marquee_rect: Recti,
    marquee_rect_text: String,

    show_background_icon: OnceCell<egui::TextureHandle>,
    show_pixel_value_icon: OnceCell<egui::TextureHandle>,
    show_crosshair_icon: OnceCell<egui::TextureHandle>,

    scale_linear_icon: OnceCell<egui::TextureHandle>,
    scale_inverse_icon: OnceCell<egui::TextureHandle>,
    scale_log_icon: OnceCell<egui::TextureHandle>,

    // Panel visibility toggles
    show_side_panel: bool,
    show_bottom_panel: bool,
}

impl ViewerApp {
    pub fn new() -> Self {
        let state = AppState::empty();
        let marquee_rect = state.marquee_rect.clone();
        let shader_params = state.shader_params.clone();

        Self {
            state,
            viewer: ImageViewer::new(),

            last_path: None,

            tmp_min_v: shader_params.min_v.to_string().into(),
            tmp_max_v: shader_params.max_v.to_string().into(),
            tmp_marquee_rect: marquee_rect.clone(),
            marquee_rect_text: marquee_rect.to_string().into(),

            show_background_icon: OnceCell::new(),
            show_pixel_value_icon: OnceCell::new(),
            show_crosshair_icon: OnceCell::new(),

            scale_linear_icon: OnceCell::new(),
            scale_inverse_icon: OnceCell::new(),
            scale_log_icon: OnceCell::new(),

            show_side_panel: true,
            show_bottom_panel: true,
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

        let show_background_icon = self
            .show_background_icon
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "show_background", res::icons::SHOW_BACKGROUND));
        let show_pixel_value_icon = self
            .show_pixel_value_icon
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "show_pixel_value", res::icons::SHOW_PIXEL_VALUE));
        let show_crosshair_icon = self
            .show_crosshair_icon
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "show_crosshair", res::icons::SHOW_CROSSHAIR));
        let scale_linear_icon = self
            .scale_linear_icon
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "scale_linear", res::icons::SCALE_LINEAR));
        let scale_inverse_icon = self
            .scale_inverse_icon
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "scale_inverse", res::icons::SCALE_INVERSE));
        let scale_log_icon = self
            .scale_log_icon
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "scale_log", res::icons::SCALE_LOG));

        if self.state.path != self.last_path {
            self.last_path = self.state.path.clone();
            if let Some(p) = &self.state.path {
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} - edolview", p.display()).to_owned()));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Title("edolview".to_owned()));
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            let cur_full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!cur_full));
        }

        if !ctx.wants_keyboard_input() {
            ctx.input_mut(|i| {
                if i.consume_shortcut(&SELECT_ALL_SC) {
                    if let Some(d) = &self.state.display {
                        let spec = d.spec();
                        let img_rect = Recti::from_min_size(vec2i(0, 0), vec2i(spec.width, spec.height));
                        self.state.marquee_rect = img_rect;
                        self.tmp_marquee_rect = img_rect;
                        self.marquee_rect_text = img_rect.to_string().into();
                    }
                }
                if i.consume_shortcut(&SELECT_NONE_SC) {
                    self.state.reset_marquee_rect();
                }
                if i.consume_shortcut(&COPY_SC) {
                    self.viewer.request_copy();
                }
                if i.key_pressed(egui::Key::ArrowLeft) {
                    if let Err(e) = self.state.navigate_prev() {
                        eprintln!("Failed to navigate prev: {e}");
                    } else {
                        self.viewer.reset_view();
                    }
                }
                if i.key_pressed(egui::Key::ArrowRight) {
                    if let Err(e) = self.state.navigate_next() {
                        eprintln!("Failed to navigate next: {e}");
                    } else {
                        self.viewer.reset_view();
                    }
                }
            });
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

        self.state.validate_marquee_rect();
        self.state.process_watcher_events();

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

                if ui.button("Fit Selection").on_hover_text("Fit marquee to view").clicked() {
                    let rect = self.state.marquee_rect.validate();
                    if rect.empty() {
                        if let Some(d) = &self.state.display {
                            let spec = d.spec();
                            let img_rect = Recti::from_min_size(vec2i(0, 0), vec2i(spec.width, spec.height));
                            self.viewer.fit_rect(img_rect);
                        }
                    } else {
                        self.viewer.fit_rect(rect);
                    }
                }
                if ui.button("Center Selection").on_hover_text("Center marquee in view").clicked() {
                    let rect = self.state.marquee_rect.validate();
                    if rect.empty() {
                        if let Some(d) = &self.state.display {
                            let spec = d.spec();
                            let img_rect = Recti::from_min_size(vec2i(0, 0), vec2i(spec.width, spec.height));
                            self.viewer.center_rect(img_rect);
                        }
                    } else {
                        self.viewer.center_rect(rect);
                    }
                }

                ui.separator();
                ui.checkbox(&mut self.state.copy_use_original_size, "Copy at original size")
                    .on_hover_text("When enabled, Ctrl+C copies marquee at image pixel size (ignores zoom).");
                ui.toggle_icon(
                    &mut self.state.is_show_background,
                    show_background_icon.to_icon(ui),
                    "Show Background",
                );
                ui.toggle_icon(
                    &mut self.state.is_show_pixel_value,
                    show_pixel_value_icon.to_icon(ui),
                    "Show Pixel Value",
                );
                ui.toggle_icon(
                    &mut self.state.is_show_crosshair,
                    show_crosshair_icon.to_icon(ui),
                    "Show Crosshair",
                );

                // Right end: panel visibility toggles
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut self.show_bottom_panel, "Status Bar");
                    ui.toggle_value(&mut self.show_side_panel, "Sidebar");
                });
            });
        });

        if self.show_bottom_panel {
            egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
                ui.columns_sized(
                    [
                        Size::exact(320.0),
                        Size::exact(160.0),
                        Size::remainder(1.0),
                        Size::exact(64.0),
                    ],
                    |columns| {
                        columns[0].vertical(|ui| {
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

                                let rect = self.state.marquee_rect;
                                let mean_color = if let Ok(mean) = d.mean_value_in_rect(rect.to_cv_rect()) {
                                    mean.iter().map(|&v| v as f32).collect()
                                } else {
                                    vec![0.0; d.spec().channels as usize]
                                };
                                ui.label_with_colored_rect(mean_color, dtype);
                            }
                        });

                        columns[1].vertical(|ui| {
                            if let Some(cursor_pos) = self.state.cursor_pos {
                                ui.label(format!("Cursor: ({}, {})", cursor_pos.x, cursor_pos.y));
                            } else {
                                ui.label("Cursor: (-, -)");
                            }

                            ui.text_edit_value_capture(
                                &mut self.marquee_rect_text,
                                &mut self.state.marquee_rect,
                                &mut self.tmp_marquee_rect,
                            );
                        });

                        columns[3].with_layout(egui::Layout::top_down(egui::Align::RIGHT), |ui| {
                            ui.label("Zoom:");
                            ui.label(format!("{:.2}x", self.viewer.zoom()));
                        });
                    },
                );
            });
        }

        if self.show_side_panel {
            egui::SidePanel::right("right")
                .resizable(true)
                .width_range(Rangef::new(240.0, f32::INFINITY))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("View Settings");
                    });
                    ui.style_mut().spacing.slider_rail_height = 4.0;

                    let channels = self.state.display.as_ref().map(|d| d.spec().channels).unwrap_or(0);
                    let is_mono = self.state.channel_index != -1 || channels == 1;
                    let visible_channels = if is_mono { 1 } else { channels.max(1).min(4) } as usize;

                    ui.separator();
                    ui.checkbox(&mut self.state.shader_params.use_per_channel, "Per-channel controls");

                    ui.vertical(|ui| {
                        let mut style: egui::Style = ui.style().as_ref().clone();
                        egui::containers::menu::menu_style(&mut style);
                        ui.set_style(std::sync::Arc::new(style));

                        if self.state.shader_params.use_per_channel {
                            for i in 0..visible_channels {
                                ui.horizontal(|ui| {
                                    let label = match (visible_channels, i) {
                                        (1, 0) => "C".to_string(),
                                        (_, 0) => "R".to_string(),
                                        (_, 1) => "G".to_string(),
                                        (_, 2) => "B".to_string(),
                                        _ => "A".to_string(),
                                    };
                                    ui.label(format!("{}", label));
                                    ui.radio_icon(
                                        &mut self.state.shader_params.scale_mode_channels[i],
                                        ScaleMode::Linear,
                                        scale_linear_icon.to_icon(ui),
                                        "Linear",
                                    );
                                    ui.radio_icon(
                                        &mut self.state.shader_params.scale_mode_channels[i],
                                        ScaleMode::Inverse,
                                        scale_inverse_icon.to_icon(ui),
                                        "Inverse",
                                    );

                                    ui.separator();

                                    ui.style_mut().spacing.item_spacing = vec2(0.0, 0.0);
                                    ui.columns_sized(
                                        [Size::remainder(1.0), Size::exact(24.0), Size::remainder(1.0)],
                                        |columns| {
                                            columns[0]
                                                .add(
                                                    egui::DragValue::new(
                                                        &mut self.state.shader_params.min_v_channels[i],
                                                    )
                                                    .speed(0.01)
                                                )
                                                .on_hover_text("Min value");
                                            if columns[1]
                                                .button("↔")
                                                .on_hover_text("Switch min/max for this channel")
                                                .clicked()
                                            {
                                                std::mem::swap(
                                                    &mut self.state.shader_params.min_v_channels[i],
                                                    &mut self.state.shader_params.max_v_channels[i],
                                                );
                                            }
                                            columns[2]
                                                .add(
                                                    egui::DragValue::new(
                                                        &mut self.state.shader_params.max_v_channels[i],
                                                    )
                                                    .speed(0.01)
                                                )
                                                .on_hover_text("Max value");
                                        },
                                    );
                                });
                            }
                        } else {
                            ui.horizontal(|ui| {
                                ui.radio_icon(
                                    &mut self.state.shader_params.scale_mode,
                                    ScaleMode::Linear,
                                    scale_linear_icon.to_icon(ui),
                                    "Linear",
                                );
                                ui.radio_icon(
                                    &mut self.state.shader_params.scale_mode,
                                    ScaleMode::Inverse,
                                    scale_inverse_icon.to_icon(ui),
                                    "Inverse",
                                );

                                ui.separator();

                                ui.style_mut().spacing.item_spacing = vec2(0.0, 0.0);
                                ui.columns_sized(
                                    [Size::remainder(1.0), Size::exact(24.0), Size::remainder(1.0)],
                                    |columns| {
                                        columns[0]
                                            .add(egui::DragValue::new(&mut self.state.shader_params.min_v).speed(0.01))
                                            .on_hover_text("Min value");

                                        if columns[1].button("↔").on_hover_text("Switch min/max").clicked() {
                                            std::mem::swap(
                                                &mut self.state.shader_params.min_v,
                                                &mut self.state.shader_params.max_v,
                                            );
                                            self.tmp_min_v = format!("{}", self.state.shader_params.min_v);
                                            self.tmp_max_v = format!("{}", self.state.shader_params.max_v);
                                        }
                                        columns[2]
                                            .add(egui::DragValue::new(&mut self.state.shader_params.max_v).speed(0.01))
                                            .on_hover_text("Max value");
                                    },
                                );
                            });
                        }
                    });

                    ui.separator();

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
                        ).response.on_hover_text("Channel to display -1: color, 0: R, 1: G, 2: B, 3: A");

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
                        }.response.on_hover_text("Colormap");
                    });
                });
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(0))
            .show(ctx, |ui| {
                self.viewer.show_image(ui, frame, &mut self.state);
            });
    }
}
