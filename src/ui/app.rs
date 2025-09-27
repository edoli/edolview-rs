use core::f32;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{atomic::Ordering, mpsc, Arc},
    thread,
};

use eframe::egui::{self, Color32, ModifierNames, Rangef, Visuals};
use rfd::FileDialog;

use crate::{
    model::{start_server_with_retry, AppState, Image, Recti, SocketAsset},
    res::icons::Icons,
    ui::{
        component::{
            display_controls_ui, display_profile_slider,
            egui_ext::{ComboBoxExt, Size, UiExt},
        },
        ImageViewer,
    },
    util::{cv_ext::CvIntExt, math_ext::vec2i},
};

const IS_MAC: bool = cfg!(target_os = "macos");

const SELECT_ALL_SC: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::A);
const SELECT_NONE_SC: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Escape);
const COPY_SC: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::D);

pub struct ViewerApp {
    state: AppState,
    viewer: ImageViewer,
    last_path: Option<PathBuf>,
    tmp_marquee_rect: Recti,
    marquee_rect_text: String,
    tmp_is_receiving: bool,

    // Panel visibility toggles
    show_side_panel: bool,
    show_bottom_panel: bool,

    icons: Icons,

    rx: mpsc::Receiver<SocketAsset>,
}

impl ViewerApp {
    pub fn new() -> Self {
        let state = AppState::empty();
        let marquee_rect = state.marquee_rect.clone();

        // Start socket server for receiving images
        let host = "127.0.0.1";
        let port = 21734;
        let (tx, rx) = mpsc::channel::<SocketAsset>();
        let socket_state = state.socket_state.clone();
        let socket_info = state.socket_info.clone();

        thread::spawn(move || {
            start_server_with_retry(host, port, tx, socket_state, socket_info).unwrap_or_else(|e| {
                eprintln!("Failed to start socket server: {e}");
            });
        });

        Self {
            state,
            viewer: ImageViewer::new(),

            last_path: None,

            tmp_marquee_rect: marquee_rect.clone(),
            marquee_rect_text: marquee_rect.to_string().into(),
            tmp_is_receiving: false,

            show_side_panel: true,
            show_bottom_panel: true,

            icons: Icons::new(),

            rx,
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
                    if let Some(asset) = &self.state.asset {
                        let spec = asset.image().spec();
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
                    }
                }
                if i.key_pressed(egui::Key::ArrowRight) {
                    if let Err(e) = self.state.navigate_next() {
                        eprintln!("Failed to navigate next: {e}");
                    }
                }
                if i.key_pressed(egui::Key::R) {
                    self.viewer.reset_view();
                }
                if i.key_pressed(egui::Key::Plus) {
                    self.viewer.zoom_in(1.0, None);
                }
                if i.key_pressed(egui::Key::Minus) {
                    self.viewer.zoom_in(-1.0, None);
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
                        Ok(_) => {}
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

                if ui.button("Clipboard").on_hover_text("Load image from clipboard").clicked() {
                    self.state.load_from_clipboard().unwrap_or_else(|e| {
                        eprintln!("Failed to load image from clipboard: {e}");
                    });
                }

                ui.separator();
                if ui
                    .button("Reset View")
                    .on_hover_text("Reset zoom and pan to original")
                    .clicked()
                {
                    self.viewer.reset_view();
                }

                if ui.button("Fit Selection").on_hover_text("Fit marquee to view").clicked() {
                    let rect = self.state.marquee_rect.validate();
                    if rect.empty() {
                        if let Some(asset) = &self.state.asset {
                            let spec = asset.image().spec();
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
                        if let Some(asset) = &self.state.asset {
                            let spec = asset.image().spec();
                            let img_rect = Recti::from_min_size(vec2i(0, 0), vec2i(spec.width, spec.height));
                            self.viewer.center_rect(img_rect);
                        }
                    } else {
                        self.viewer.center_rect(rect);
                    }
                }

                ui.separator();
                ui.checkbox(&mut self.state.copy_use_original_size, "Copy at original size")
                    .on_hover_text(format!(
                        "When enabled, {} copies marquee at image pixel size (ignores zoom).",
                        COPY_SC.format(&ModifierNames::NAMES, IS_MAC)
                    ));
                ui.toggle_icon(
                    &mut self.state.is_show_background,
                    self.icons.get_show_background(ctx),
                    "Show Background",
                );
                ui.toggle_icon(
                    &mut self.state.is_show_pixel_value,
                    self.icons.get_show_pixel_value(ctx),
                    "Show Pixel Value",
                );
                ui.toggle_icon(
                    &mut self.state.is_show_crosshair,
                    self.icons.get_show_crosshair(ctx),
                    "Show Crosshair",
                );

                ui.visuals_mut().override_text_color = Some(ui.visuals().weak_text_color());
                let socket_address = self.state.socket_info.lock().unwrap().address.clone();
                ui.label(socket_address.clone())
                    .on_hover_text("Socket Listener Address")
                    .context_menu(|ui| {
                        if ui.button("Copy Address").clicked() {
                            arboard::Clipboard::new()
                                .and_then(|mut cb| cb.set_text(socket_address))
                                .unwrap_or_else(|e| {
                                    eprintln!("Failed to copy socket address to clipboard: {e}");
                                });
                            ui.close();
                        }
                    });
                ui.visuals_mut().override_text_color = None;
                ui.indicator_icon(
                    self.state.socket_state.is_socket_receiving.load(Ordering::Relaxed),
                    self.icons.get_downloading(ctx),
                    "Image Receiving",
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
                        Size::exact(400.0),
                        Size::exact(160.0),
                        Size::remainder(1.0),
                        Size::exact(128.0),
                    ],
                    |columns| {
                        columns[0].vertical(|ui| {
                            if let Some(asset) = &self.state.asset {
                                let image = asset.image();
                                let spec = image.spec();
                                let dtype = image.spec().dtype;

                                let cursor_color = if let Ok(pixel) = image.get_pixel_at(
                                    self.state.cursor_pos.map_or(-1, |p| p.x),
                                    self.state.cursor_pos.map_or(-1, |p| p.y),
                                ) {
                                    pixel.iter().cloned().collect()
                                } else {
                                    vec![0.0; spec.channels as usize]
                                };
                                ui.label_with_colored_rect(cursor_color, dtype);

                                let rect = self.state.marquee_rect;
                                let mean_color = if let Ok(mean) = image.mean_value_in_rect(rect.to_cv_rect()) {
                                    mean.iter().map(|&v| v as f32).collect()
                                } else {
                                    vec![0.0; image.spec().channels as usize]
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
                            )
                            .context_menu(|ui| {
                                if ui.button("Copy Numpy Indexing").clicked() {
                                    let rect = self.state.marquee_rect.validate();
                                    let np_indexing =
                                        format!("{}:{}, {}:{}", rect.min.y, rect.max.y, rect.min.x, rect.max.x,);
                                    arboard::Clipboard::new()
                                        .and_then(|mut cb| cb.set_text(np_indexing))
                                        .unwrap_or_else(|e| {
                                            eprintln!("Failed to copy numpy indexing to clipboard: {e}");
                                        });
                                    ui.close();
                                }
                                if ui.button("Copy x1, y1, x2, y2").clicked() {
                                    let rect = self.state.marquee_rect.validate();
                                    let rect_str =
                                        format!("{}, {}, {}, {}", rect.min.x, rect.min.y, rect.max.x, rect.max.y);
                                    arboard::Clipboard::new()
                                        .and_then(|mut cb| cb.set_text(rect_str))
                                        .unwrap_or_else(|e| {
                                            eprintln!("Failed to copy rect to clipboard: {e}");
                                        });
                                    ui.close();
                                }
                            });
                        });

                        columns[3].with_layout(egui::Layout::top_down(egui::Align::RIGHT), |ui| {
                            if let Some(asset) = &self.state.asset {
                                let spec = asset.image().spec();
                                ui.label(format!("{}×{} | {}", spec.width, spec.height, spec.dtype.cv_type_name()));
                            } else {
                                ui.label("No image loaded");
                            }
                            ui.label(format!("Zoom: {:.2}x", self.viewer.zoom()));
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

                    let channels = self.state.asset.as_ref().map(|a| a.image().spec().channels).unwrap_or(0);
                    let is_mono = self.state.channel_index != -1 || channels == 1;

                    ui.horizontal(|ui| {
                        let sizes = ui.calc_sizes([Size::exact(50.0), Size::remainder(1.0)]);
                        ui.spacing_mut().combo_width = sizes[0];
                        let channel_values: Vec<i32> = (-1..channels).collect();
                        egui::ComboBox::from_id_salt("channel_index")
                            .combo_i32_with(ui, &mut self.state.channel_index, &channel_values, |v| match v {
                                -1 => "All".to_string(),
                                0 => {
                                    if channels > 1 {
                                        "R".to_string()
                                    } else {
                                        "C".to_string()
                                    }
                                }
                                1 => "G".to_string(),
                                2 => "B".to_string(),
                                3 => "A".to_string(),
                                _ => format!("C{}", v),
                            })
                            .response
                            .on_hover_text("Channel to display");

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
                        }
                        .response
                        .on_hover_text("Colormap");
                    });

                    ui.separator();
                    ui.checkbox(&mut self.state.shader_params.use_per_channel, "Per-channel controls");

                    if let Some(asset) = &self.state.asset {
                        let image = asset.image();
                        ui.vertical(|ui| {
                            let mut style: egui::Style = ui.style().as_ref().clone();
                            egui::containers::menu::menu_style(&mut style);
                            ui.set_style(std::sync::Arc::new(style));

                            if self.state.shader_params.use_per_channel {
                                for i in 0..channels {
                                    ui.horizontal(|ui| {
                                        let label = match (channels, i) {
                                            (1, 0) => "C".to_string(),
                                            (_, 0) => "R".to_string(),
                                            (_, 1) => "G".to_string(),
                                            (_, 2) => "B".to_string(),
                                            _ => "A".to_string(),
                                        };
                                        ui.label(format!("{}", label));

                                        display_controls_ui(
                                            ui,
                                            &self.icons,
                                            image,
                                            i,
                                            &mut self.state.shader_params.scale_mode_channels[i as usize],
                                            &mut self.state.shader_params.auto_minmax_channels[i as usize],
                                            &mut self.state.shader_params.min_v_channels[i as usize],
                                            &mut self.state.shader_params.max_v_channels[i as usize],
                                        );
                                    });
                                }
                            } else {
                                ui.horizontal(|ui| {
                                    display_controls_ui(
                                        ui,
                                        &self.icons,
                                        image,
                                        -1,
                                        &mut self.state.shader_params.scale_mode,
                                        &mut self.state.shader_params.auto_minmax,
                                        &mut self.state.shader_params.min_v,
                                        &mut self.state.shader_params.max_v,
                                    );
                                });
                            }
                        });
                    }

                    ui.separator();

                    display_profile_slider(ui, &mut self.state.shader_params.offset, -5.0, 5.0, 0.0, "Offset");
                    display_profile_slider(ui, &mut self.state.shader_params.exposure, -5.0, 5.0, 0.0, "Exposure");
                    display_profile_slider(ui, &mut self.state.shader_params.gamma, 0.1, 5.0, 1.0, "Gamma");

                    ui.separator();

                    ui.heading("Image List");
                    let asset = self.state.asset.clone();
                    let asset_hash = if let Some(asset) = asset {
                        Some(&asset.hash().to_string())
                    } else {
                        None
                    };

                    egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                        let mut to_set: Option<_> = None;
                        let mut to_remove: HashSet<_> = HashSet::new();
                        let mut to_retain: HashSet<_> = HashSet::new();

                        self.state.assets.iter().for_each(|(hash, asset)| {
                            let name = asset.name();

                            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                                let btn = if Some(hash) == asset_hash {
                                    ui.selectable_label(true, name)
                                } else {
                                    ui.selectable_label(false, name)
                                };
                                btn.context_menu(|ui| {
                                    ui.visuals_mut().override_text_color = Some(Color32::from_rgb(255, 100, 100));
                                    if ui.button("Delete").clicked() {
                                        to_remove.insert(hash.clone());
                                        ui.close();
                                    }
                                    if ui.button("Delete Others").clicked() {
                                        to_retain.insert(hash.clone());
                                        ui.close();
                                    }
                                    ui.visuals_mut().override_text_color = None;

                                    match asset.asset_type() {
                                        crate::model::AssetType::File => {
                                            if ui.button("Copy Path").clicked() {
                                                let path = asset.name();
                                                arboard::Clipboard::new()
                                                    .and_then(|mut cb| cb.set_text(path.to_string()))
                                                    .unwrap_or_else(|e| {
                                                        eprintln!("Failed to copy path to clipboard: {e}");
                                                    });
                                                ui.close();
                                            }
                                            if ui.button("Reveal in File Explorer").clicked() {
                                                let path = asset.name();
                                                let path_buf = PathBuf::from(path);
                                                if let Err(e) = opener::open(
                                                    path_buf.parent().unwrap_or_else(|| std::path::Path::new(".")),
                                                ) {
                                                    eprintln!("Failed to open file explorer: {e}");
                                                }
                                                ui.close();
                                            }
                                        }
                                        crate::model::AssetType::Clipboard => {}
                                        _ => {}
                                    }
                                });
                                if btn.clicked() {
                                    to_set = Some(asset.clone());
                                }
                            });
                        });

                        if let Some(to_set) = to_set {
                            self.state.set_asset(to_set);
                        }

                        if to_retain.is_empty() {
                            self.state.assets.retain(|hash, _| !to_remove.contains(hash));
                        } else {
                            self.state.assets.retain(|hash, _| to_retain.contains(hash));
                        }

                        if let Some(asset) = &self.state.asset {
                            if !self.state.assets.contains_key(asset.hash()) {
                                self.state.clear_asset();
                            }
                        }
                    });
                });
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(0))
            .show(ctx, |ui| {
                self.viewer.show_image(ui, frame, &mut self.state);
            });

        // Debug window
        #[cfg(debug_assertions)]
        {
            crate::debug::debug_window(ctx);
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, _raw_input: &mut egui::RawInput) {
        let current_is_receiving = self.state.socket_state.is_socket_receiving.load(Ordering::Relaxed);
        if self.tmp_is_receiving != current_is_receiving {
            _ctx.request_repaint();
            self.tmp_is_receiving = current_is_receiving;
        }

        match self.rx.try_recv() {
            Ok(asset) => {
                self.state.set_asset(Arc::new(asset));
                _ctx.request_repaint();
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }
}
