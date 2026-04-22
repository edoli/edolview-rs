use color_eyre::eyre::{eyre, Result};
use eframe::egui::{self, vec2};
use eframe::glow::{self as GL, HasContext};
use opencv::{core, imgcodecs, imgproc, prelude::*};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::model::{AppState, Image, MeanDim, Recti, EMPTY_MINMAX};
use crate::res::KeyboardShortcutExt;
use crate::ui::gl::{BackgroundProgram, ImageProgram};
use crate::util::cv_ext::CvIntExt;
use crate::util::func_ext::FuncExt;
use crate::util::math_ext::vec2i;

enum ExportToast {
    Success(String),
    Error(String),
}

enum DragMode {
    None,
    Panning {
        last_pixel_pos: egui::Pos2,
    },
    Marquee {
        start_image_pos: egui::Pos2,
    },
    Resizing {
        handle: ResizeHandle,
        start_rect: Recti,
        start_pointer_image_pos: egui::Pos2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResizeHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

pub struct ImageViewer {
    background_prog: Option<Arc<BackgroundProgram>>,
    image_prog: Option<Arc<Mutex<ImageProgram>>>,
    gl_primary_tex: Option<GL::NativeTexture>,
    gl_secondary_tex: Option<GL::NativeTexture>,
    zoom_level: f32,
    zoom_base: f32,
    pan: egui::Vec2,
    dragging: bool,
    drag_mode: DragMode,
    copy_requested: Option<String>,
    save_dialog_requested: bool,
    save_requested: Option<(PathBuf, String)>,
    export_toasts: Arc<Mutex<Vec<ExportToast>>>,
    last_primary_image_id: Option<u64>, // cache key to know when to re-upload texture
    last_secondary_image_id: Option<u64>,
    last_viewport_size_px: Option<egui::Vec2>,

    last_shader_error: Option<String>,
    last_reported_shader_error: Option<String>,
}

impl ImageViewer {
    pub fn new() -> Self {
        Self {
            background_prog: None,
            image_prog: None,
            gl_primary_tex: None,
            gl_secondary_tex: None,
            zoom_level: 0.0,
            zoom_base: 2.0_f32.powf(1.0 / 4.0),
            pan: egui::Vec2::ZERO,
            dragging: false,
            drag_mode: DragMode::None,
            copy_requested: None,
            save_dialog_requested: false,
            save_requested: None,
            export_toasts: Arc::new(Mutex::new(Vec::new())),
            last_primary_image_id: None,
            last_secondary_image_id: None,
            last_viewport_size_px: None,
            last_shader_error: None,
            last_reported_shader_error: None,
        }
    }

    pub fn show_image(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame, app_state: &mut AppState) {
        self.refresh_shader_error();
        let Some(asset) = app_state.asset.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label("Drag & Drop an image file here.");
            });
            return;
        };
        let split_view = self.is_split_comparison(app_state);
        let primary_asset = app_state.asset_primary.clone().unwrap_or_else(|| asset.clone());
        let secondary_asset = if split_view {
            app_state.asset_secondary.clone()
        } else {
            None
        };
        let primary_image = primary_asset.image();
        let secondary_image = secondary_asset.as_ref().map(|a| a.image());
        let image = asset.image();
        let render_primary_image = if split_view { primary_image } else { image };

        // Determine if we need a (re)upload
        let spec = render_primary_image.spec();
        let use_auto_minmax = app_state.shader_params.auto_minmax && !app_state.shader_params.use_per_channel
            || app_state.shader_params.auto_minmax_channels.iter().any(|&b| b);
        let min_max_primary = if use_auto_minmax {
            render_primary_image.minmax().clone()
        } else {
            EMPTY_MINMAX
        };
        let min_max_secondary = if use_auto_minmax {
            secondary_image
                .map(|secondary_image| secondary_image.minmax().clone())
                .unwrap_or_else(|| min_max_primary.clone())
        } else {
            EMPTY_MINMAX
        };

        if let Some(gl) = frame.gl() {
            sync_mat_texture(
                gl,
                &mut self.gl_primary_tex,
                &mut self.last_primary_image_id,
                render_primary_image,
            );
            if let Some(secondary_image) = secondary_image {
                sync_mat_texture(
                    gl,
                    &mut self.gl_secondary_tex,
                    &mut self.last_secondary_image_id,
                    secondary_image,
                );
            } else {
                release_texture(gl, &mut self.gl_secondary_tex, &mut self.last_secondary_image_id);
            }
        }

        if self.gl_primary_tex.is_some() && (!split_view || self.gl_secondary_tex.is_some()) {
            if self.background_prog.is_none() {
                if let Some(gl) = frame.gl() {
                    if let Ok(p) = BackgroundProgram::new(gl) {
                        self.background_prog = Some(Arc::new(p));
                    }
                }
            }

            if self.image_prog.is_none() {
                if let Some(gl) = frame.gl() {
                    match ImageProgram::new(gl) {
                        Ok(p) => {
                            self.image_prog = Some(Arc::new(Mutex::new(p)));
                        }
                        Err(e) => {
                            self.last_shader_error = Some(e.to_string());
                        }
                    }
                }
            }

            let available_points = ui.available_size();
            // Enable both drag (for panning / marquee) and click (for context menu)
            let (rect, resp) = ui.allocate_exact_size(available_points, egui::Sense::click_and_drag());
            let pixel_per_point = ui.ctx().pixels_per_point();
            let rect_pixels = rect * pixel_per_point;

            // Record viewport size in pixels for fit/center operations triggered from menus
            self.last_viewport_size_px = Some(vec2(rect_pixels.width(), rect_pixels.height()));
            let (left_pane_rect, right_pane_rect) = self.split_pane_rects(rect);
            let active_primary_rect = if split_view { left_pane_rect } else { rect };

            // Pre-compute selection rect in view space (points) for handle interactions
            let selection_rect_view =
                self.selection_rect_in_view(active_primary_rect, app_state.marquee_rect, pixel_per_point);
            let secondary_selection_rect_view = split_view
                .then(|| self.selection_rect_in_view(right_pane_rect, app_state.marquee_rect, pixel_per_point));
            let selection_rect_primary_clipped = selection_rect_view.intersect(active_primary_rect);
            let selection_rect_secondary_clipped =
                secondary_selection_rect_view.map(|selection_rect_view| selection_rect_view.intersect(right_pane_rect));

            // Detect if the built-in context menu popup for this response is open
            let context_menu_open = resp.context_menu_opened();

            if resp.hovered() && !context_menu_open {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll.abs() > 0.0 {
                    // Compute old scale before applying zoom change
                    let scroll_sign = scroll.signum();
                    if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                        let pane_rect = if split_view && right_pane_rect.contains(pointer) {
                            right_pane_rect
                        } else if split_view {
                            left_pane_rect
                        } else {
                            rect
                        };
                        let local = (pointer - pane_rect.min) * pixel_per_point;
                        self.zoom_in(scroll_sign, Some(local));
                    }
                }

                let mouse_pos = ui.input(|i| i.pointer.hover_pos());
                if let Some(pointer_pos) = mouse_pos {
                    let (image_pos, cursor_on_secondary) =
                        self.view_to_image_coords(pointer_pos, rect, pixel_per_point, split_view);
                    let pixel_pos = vec2i(image_pos.x as i32, image_pos.y as i32);
                    // Check if coordinates are within image bounds
                    if pixel_pos.x >= 0 && pixel_pos.x < spec.width && pixel_pos.y >= 0 && pixel_pos.y < spec.height {
                        app_state.cursor_pos = Some(pixel_pos);
                        app_state.cursor_on_secondary = cursor_on_secondary;
                    } else {
                        app_state.cursor_pos = None;
                        app_state.cursor_on_secondary = split_view && cursor_on_secondary;
                    }

                    // If marquee exists, set resize cursor when hovering corner handles
                    if app_state.marquee_rect.width() > 0 && app_state.marquee_rect.height() > 0 {
                        let handle = hit_test_resize_handle_with_rects(
                            selection_rect_view,
                            selection_rect_primary_clipped,
                            secondary_selection_rect_view,
                            selection_rect_secondary_clipped,
                            pointer_pos,
                        )
                        .map(|(handle, _, _)| handle);
                        if let Some(handle) = handle {
                            let icon = match handle {
                                ResizeHandle::TopLeft | ResizeHandle::BottomRight => egui::CursorIcon::ResizeNwSe,
                                ResizeHandle::TopRight | ResizeHandle::BottomLeft => egui::CursorIcon::ResizeNeSw,
                            };
                            ui.output_mut(|o| o.cursor_icon = icon);
                        }
                    }
                }
            }

            // Context menu (right-click)
            resp.context_menu(|ui| {
                let active_image = if split_view && app_state.cursor_on_secondary {
                    secondary_image.unwrap_or(primary_image)
                } else if split_view {
                    primary_image
                } else {
                    image
                };
                let has_selection = !app_state.marquee_rect.empty();
                let copy_label = if has_selection {
                    format!("Copy Selected Image ({})", crate::res::COPY_SC.format_sys())
                } else {
                    format!("Copy Image ({})", crate::res::COPY_SC.format_sys())
                };
                if ui.button(copy_label).clicked() {
                    self.request_copy(if split_view && app_state.cursor_on_secondary {
                        "secondary".to_string()
                    } else if split_view {
                        "primary".to_string()
                    } else {
                        "image".to_string()
                    });
                    ui.ctx().request_repaint();
                    ui.close();
                }
                let save_label = if has_selection {
                    format!("Save Selected Image As... ({})", crate::res::SAVE_IMAGE_SC.format_sys())
                } else {
                    format!("Save Image As... ({})", crate::res::SAVE_IMAGE_SC.format_sys())
                };
                if ui.button(save_label).clicked() {
                    self.request_save_dialog();
                    ui.close();
                }
                ui.separator();
                if ui.button("Copy Cursor Color").clicked() {
                    if let Some(cursor_pos) = app_state.cursor_pos {
                        if let Ok(vals) = active_image.get_pixel_at(cursor_pos.x, cursor_pos.y) {
                            let spec = active_image.spec();
                            let text = spec.pixel_values_to_string(vals);
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(text);
                            }
                        }
                    }
                    ui.close();
                }
                if ui.button("Copy Cursor Position").clicked() {
                    if let Some(cursor_pos) = app_state.cursor_pos {
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            let _ = cb.set_text(format!("{}, {}", cursor_pos.x, cursor_pos.y));
                        }
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button("Copy Rect Mean Color").clicked() {
                    if let Ok(vals) = active_image.mean_value_in_rect(app_state.marquee_rect.to_cv_rect(), MeanDim::All)
                    {
                        let spec = active_image.spec();
                        let text = spec.pixel_values_to_string(&vals);
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            let _ = cb.set_text(text);
                        }
                    }
                    ui.close();
                }
                if ui.button("Copy Rect Bound").clicked() {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(app_state.marquee_rect.to_string());
                    }
                    ui.close();
                }
            });

            // Begin interactions
            // 1) If the primary mouse button was just pressed on a handle, start resizing immediately (no drag threshold).
            if !self.dragging && resp.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
                if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                    // If a marquee exists and a corner handle is pressed, start resizing right away
                    let handle_under_mouse =
                        if app_state.marquee_rect.width() > 0 && app_state.marquee_rect.height() > 0 {
                            hit_test_resize_handle_with_rects(
                                selection_rect_view,
                                selection_rect_primary_clipped,
                                secondary_selection_rect_view,
                                selection_rect_secondary_clipped,
                                pos,
                            )
                        } else {
                            None
                        };

                    if let Some((handle, selection_rect_view, selection_rect_clipped)) = handle_under_mouse {
                        self.dragging = true;
                        self.drag_mode = DragMode::Resizing {
                            handle,
                            start_rect: self.resize_start_rect(
                                app_state.marquee_rect,
                                handle,
                                selection_rect_view,
                                selection_rect_clipped,
                                rect,
                                pixel_per_point,
                                split_view,
                            ),
                            start_pointer_image_pos: self
                                .view_to_image_coords(pos, rect, pixel_per_point, split_view)
                                .0,
                        };
                    }
                }
            }

            if !self.dragging && resp.drag_started() {
                self.dragging = true;
                if let Some(pos) = resp.interact_pointer_pos() {
                    // If a marquee exists and a corner handle is grabbed, start resizing
                    let handle_under_mouse =
                        if app_state.marquee_rect.width() > 0 && app_state.marquee_rect.height() > 0 {
                            hit_test_resize_handle_with_rects(
                                selection_rect_view,
                                selection_rect_primary_clipped,
                                secondary_selection_rect_view,
                                selection_rect_secondary_clipped,
                                pos,
                            )
                        } else {
                            None
                        };

                    self.drag_mode =
                        if let Some((handle, selection_rect_view, selection_rect_clipped)) = handle_under_mouse {
                            DragMode::Resizing {
                                handle,
                                start_rect: self.resize_start_rect(
                                    app_state.marquee_rect,
                                    handle,
                                    selection_rect_view,
                                    selection_rect_clipped,
                                    rect,
                                    pixel_per_point,
                                    split_view,
                                ),
                                start_pointer_image_pos: self
                                    .view_to_image_coords(pos, rect, pixel_per_point, split_view)
                                    .0,
                            }
                        } else if ui.input(|i| i.modifiers.shift) {
                            // Start marquee creation
                            DragMode::Marquee {
                                start_image_pos: self.view_to_image_coords(pos, rect, pixel_per_point, split_view).0,
                            }
                        } else {
                            // Start panning
                            DragMode::Panning {
                                last_pixel_pos: pos * pixel_per_point,
                            }
                        };
                }
            }

            if self.dragging {
                let pos_opt = resp.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos()));
                if let Some(pos) = pos_opt {
                    if let DragMode::Marquee { start_image_pos } = self.drag_mode {
                        // If Ctrl pressed, constrain to square relative to start
                        let is_ctrl = ui.input(|i| i.modifiers.ctrl);
                        let image_pos = self
                            .view_to_image_coords(pos, rect, pixel_per_point, split_view)
                            .0
                            .cond_map(is_ctrl, |image_pos| enforce_square_from_anchor(start_image_pos, image_pos));
                        app_state.set_marquee_rect(Recti::bound_two_pos(start_image_pos, image_pos));
                    } else if let DragMode::Panning {
                        last_pixel_pos: last_pos,
                    } = self.drag_mode
                    {
                        let pos = pos * pixel_per_point;
                        let delta = pos - last_pos;
                        self.drag_mode = DragMode::Panning { last_pixel_pos: pos };
                        self.pan += egui::vec2(delta.x, delta.y);
                    } else if let DragMode::Resizing {
                        handle,
                        start_rect,
                        start_pointer_image_pos,
                    } = self.drag_mode
                    {
                        // Compute delta in image space from the initial press
                        let curr_img = self.view_to_image_coords(pos, rect, pixel_per_point, split_view).0;
                        let delta =
                            egui::vec2(curr_img.x - start_pointer_image_pos.x, curr_img.y - start_pointer_image_pos.y);

                        // Prepare moving and anchor corners in image space (f32)
                        let mut moving = match handle {
                            ResizeHandle::TopLeft => egui::pos2(start_rect.min.x as f32, start_rect.min.y as f32),
                            ResizeHandle::TopRight => egui::pos2(start_rect.max.x as f32, start_rect.min.y as f32),
                            ResizeHandle::BottomLeft => egui::pos2(start_rect.min.x as f32, start_rect.max.y as f32),
                            ResizeHandle::BottomRight => egui::pos2(start_rect.max.x as f32, start_rect.max.y as f32),
                        };
                        let anchor = match handle {
                            ResizeHandle::TopLeft => egui::pos2(start_rect.max.x as f32, start_rect.max.y as f32),
                            ResizeHandle::TopRight => egui::pos2(start_rect.min.x as f32, start_rect.max.y as f32),
                            ResizeHandle::BottomLeft => egui::pos2(start_rect.max.x as f32, start_rect.min.y as f32),
                            ResizeHandle::BottomRight => egui::pos2(start_rect.min.x as f32, start_rect.min.y as f32),
                        };

                        // Apply delta to moving corner
                        moving.x += delta.x;
                        moving.y += delta.y;

                        // If Ctrl pressed, constrain to square relative to anchor
                        let is_ctrl = ui.input(|i| i.modifiers.ctrl);
                        if is_ctrl {
                            moving = enforce_square_from_anchor(anchor, moving);
                        }

                        app_state.set_marquee_rect(Recti::bound_two_pos(anchor, moving));
                    }
                }

                // End dragging either when egui reports drag stopped, or when primary is released
                if resp.drag_stopped() || ui.input(|i| i.pointer.primary_released()) {
                    self.dragging = false;
                    self.drag_mode = DragMode::None;
                }
            }

            // Recalculate selection rect in view space (points) for drawing
            let selection_rect_view =
                self.selection_rect_in_view(active_primary_rect, app_state.marquee_rect, pixel_per_point);
            let secondary_selection_rect_view = split_view
                .then(|| self.selection_rect_in_view(right_pane_rect, app_state.marquee_rect, pixel_per_point));
            let selection_rect_primary_clipped = selection_rect_view.intersect(active_primary_rect);
            let selection_rect_secondary_clipped =
                secondary_selection_rect_view.map(|selection_rect_view| selection_rect_view.intersect(right_pane_rect));

            // Queue clipboard/save export operations to perform inside the GL callback.
            let mut export_request: Option<(Option<String>, Option<(PathBuf, String)>, i32, i32, egui::Vec2, f32)> =
                None;
            let copy_requested = self.copy_requested.take();
            let save_requested = self.save_requested.take();
            if copy_requested.is_some() || save_requested.is_some() {
                let export_rect = self.copy_rect(app_state, spec.width, spec.height);
                if !export_rect.empty() {
                    let scale_for_export = if app_state.copy_use_original_size {
                        1.0
                    } else {
                        self.zoom()
                    };
                    let out_w = (export_rect.width() as f32 * scale_for_export).round().max(1.0) as i32;
                    let out_h = (export_rect.height() as f32 * scale_for_export).round().max(1.0) as i32;
                    let position = egui::vec2(
                        -(export_rect.min.x as f32) * scale_for_export,
                        -(export_rect.min.y as f32) * scale_for_export,
                    );
                    export_request = Some((copy_requested, save_requested, out_w, out_h, position, scale_for_export));
                }
            }

            if let (Some(background_prog), Some(image_prog), Some(_gl)) =
                (self.background_prog.clone(), self.image_prog.clone(), frame.gl())
            {
                let viewport_size = vec2(rect_pixels.width() as f32, rect_pixels.height() as f32);
                let left_pane_pixels = left_pane_rect * pixel_per_point;
                let right_pane_pixels = right_pane_rect * pixel_per_point;
                let pane_viewport_size = vec2(left_pane_pixels.width() as f32, left_pane_pixels.height() as f32);
                let image_size = vec2(spec.width as f32, spec.height as f32);

                let primary_tex_handle = self.gl_primary_tex;
                let secondary_tex_handle = self.gl_secondary_tex;
                let scale = self.zoom() as f32;
                let position = self.pan;

                let visuals = ui.visuals().clone();
                let shader_params = app_state.shader_params.clone();
                let channel_index = app_state.channel_index;

                let is_mono = channel_index != -1 || spec.channels == 1;
                let colormap = if is_mono {
                    app_state.colormap_mono.clone()
                } else {
                    app_state.colormap_rgb.clone()
                };
                let is_show_background = app_state.is_show_background;
                let export_toasts = self.export_toasts.clone();
                let repaint_ctx = ui.ctx().clone();

                ui.painter().add(egui::PaintCallback {
                    rect,
                    callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |info, painter| unsafe {
                        let gl = painter.gl();
                        let screen_h = info.screen_size_px[1] as i32;
                        let screen_w = info.screen_size_px[0] as i32;

                        let draw_pane = |pane_pixels: egui::Rect,
                                         pane_size: egui::Vec2,
                                         tex: Option<GL::NativeTexture>,
                                         min_max: &crate::model::MinMax| {
                            let x = pane_pixels.min.x.round() as i32;
                            let y_top = pane_pixels.max.y.round() as i32;
                            let height = pane_pixels.height().round() as i32;
                            let y = screen_h - y_top;
                            let width = pane_pixels.width().round() as i32;
                            gl.viewport(x, y, width, height);

                            if is_show_background {
                                background_prog.draw(
                                    gl,
                                    pane_size,
                                    position,
                                    16.0,
                                    visuals.extreme_bg_color,
                                    visuals.faint_bg_color,
                                );
                            }

                            if let (Some(draw_tex), Ok(mut image_prog)) = (tex, image_prog.lock()) {
                                image_prog.draw(
                                    gl,
                                    draw_tex,
                                    colormap.as_str(),
                                    pane_size,
                                    image_size,
                                    channel_index,
                                    min_max,
                                    is_mono,
                                    scale,
                                    position,
                                    &shader_params,
                                );
                            }
                        };

                        draw_pane(
                            if split_view { left_pane_pixels } else { rect_pixels },
                            if split_view { pane_viewport_size } else { viewport_size },
                            primary_tex_handle,
                            &min_max_primary,
                        );
                        if split_view {
                            draw_pane(right_pane_pixels, pane_viewport_size, secondary_tex_handle, &min_max_secondary);
                        }

                        // If an export was requested, render to an offscreen FBO once and reuse the RGBA readback.
                        if let Some((copy_requested, save_requested, out_w, out_h, crop_pos, export_scale)) =
                            export_request.clone()
                        {
                            // Create offscreen target
                            let fbo = gl.create_framebuffer().unwrap();
                            gl.bind_framebuffer(GL::FRAMEBUFFER, Some(fbo));
                            let tex = gl.create_texture().unwrap();
                            gl.bind_texture(GL::TEXTURE_2D, Some(tex));
                            gl.tex_parameter_i32(GL::TEXTURE_2D, GL::TEXTURE_MIN_FILTER, GL::LINEAR as _);
                            gl.tex_parameter_i32(GL::TEXTURE_2D, GL::TEXTURE_MAG_FILTER, GL::LINEAR as _);
                            gl.tex_parameter_i32(GL::TEXTURE_2D, GL::TEXTURE_WRAP_S, GL::CLAMP_TO_EDGE as _);
                            gl.tex_parameter_i32(GL::TEXTURE_2D, GL::TEXTURE_WRAP_T, GL::CLAMP_TO_EDGE as _);
                            gl.tex_image_2d(
                                GL::TEXTURE_2D,
                                0,
                                GL::RGBA8 as i32,
                                out_w,
                                out_h,
                                0,
                                GL::RGBA,
                                GL::UNSIGNED_BYTE,
                                GL::PixelUnpackData::Slice(None),
                            );
                            gl.framebuffer_texture_2d(
                                GL::FRAMEBUFFER,
                                GL::COLOR_ATTACHMENT0,
                                GL::TEXTURE_2D,
                                Some(tex),
                                0,
                            );
                            if gl.check_framebuffer_status(GL::FRAMEBUFFER) == GL::FRAMEBUFFER_COMPLETE {
                                gl.viewport(0, 0, out_w, out_h);
                                gl.disable(GL::SCISSOR_TEST);

                                gl.clear_color(0.0, 0.0, 0.0, 0.0);
                                gl.clear(GL::COLOR_BUFFER_BIT);
                                if let Ok(mut image_prog) = image_prog.lock() {
                                    let export_secondary = save_requested
                                        .as_ref()
                                        .map(|(_, source)| source == "secondary")
                                        .or_else(|| copy_requested.as_ref().map(|source| source == "secondary"))
                                        .unwrap_or(false);
                                    let export_tex = if export_secondary {
                                        secondary_tex_handle
                                    } else {
                                        primary_tex_handle
                                    };
                                    let export_min_max = if export_secondary {
                                        &min_max_secondary
                                    } else {
                                        &min_max_primary
                                    };
                                    if let Some(draw_tex) = export_tex {
                                        image_prog.draw(
                                            gl,
                                            draw_tex,
                                            colormap.as_str(),
                                            egui::vec2(out_w as f32, out_h as f32),
                                            image_size,
                                            channel_index,
                                            export_min_max,
                                            is_mono,
                                            export_scale,
                                            crop_pos,
                                            &shader_params,
                                        );
                                    }
                                }

                                // Read back
                                let mut buf = vec![0u8; (out_w as usize) * (out_h as usize) * 4];
                                gl.read_pixels(
                                    0,
                                    0,
                                    out_w,
                                    out_h,
                                    GL::RGBA,
                                    GL::UNSIGNED_BYTE,
                                    GL::PixelPackData::Slice(Some(buf.as_mut_slice())),
                                );

                                // Flip vertically
                                let row_stride = (out_w as usize) * 4;
                                let mut flipped = vec![0u8; buf.len()];
                                for y in 0..(out_h as usize) {
                                    let src_y = (out_h as usize - 1) - y;
                                    let src_off = src_y * row_stride;
                                    let dst_off = y * row_stride;
                                    flipped[dst_off..dst_off + row_stride]
                                        .copy_from_slice(&buf[src_off..src_off + row_stride]);
                                }

                                match (copy_requested, save_requested) {
                                    (Some(copy_source), Some((path, save_source))) => {
                                        copy_image_to_clipboard(
                                            out_w,
                                            out_h,
                                            flipped.clone(),
                                            &export_toasts,
                                            copy_source.as_str(),
                                        );
                                        save_image_async(
                                            path,
                                            out_w,
                                            out_h,
                                            flipped,
                                            export_toasts.clone(),
                                            repaint_ctx.clone(),
                                            save_source,
                                        );
                                    }
                                    (Some(copy_source), None) => {
                                        copy_image_to_clipboard(
                                            out_w,
                                            out_h,
                                            flipped,
                                            &export_toasts,
                                            copy_source.as_str(),
                                        );
                                    }
                                    (None, Some((path, save_source))) => {
                                        save_image_async(
                                            path,
                                            out_w,
                                            out_h,
                                            flipped,
                                            export_toasts.clone(),
                                            repaint_ctx.clone(),
                                            save_source,
                                        );
                                    }
                                    (None, None) => {}
                                }
                            }
                            gl.bind_framebuffer(GL::FRAMEBUFFER, None);
                            gl.delete_framebuffer(fbo);
                            gl.delete_texture(tex);

                            // Restore viewport to screen
                            gl.viewport(0, 0, screen_w, screen_h);
                            gl.enable(GL::SCISSOR_TEST);
                        }
                        gl.viewport(0, 0, screen_w, screen_h);
                    })),
                });

                // Draw marquee rectangle
                let selection_rects =
                    std::iter::once((selection_rect_view, selection_rect_primary_clipped, active_primary_rect)).chain(
                        secondary_selection_rect_view.zip(selection_rect_secondary_clipped).map(
                            |(selection_rect_view, selection_rect_clipped)| {
                                (selection_rect_view, selection_rect_clipped, right_pane_rect)
                            },
                        ),
                    );
                for (selection_rect_view, selection_rect, _pane_rect) in selection_rects {
                    if selection_rect.width() <= 0.0 || selection_rect.height() <= 0.0 {
                        continue;
                    }

                    ui.painter().rect_stroke(
                        selection_rect_view,
                        0.0,
                        (1.0, egui::Color32::from_gray(150)),
                        egui::StrokeKind::Middle,
                    );

                    // Draw corner handles (small squares)
                    let painter = ui.painter();
                    let handle_size = 8.0; // in points
                    let handles = [
                        ResizeHandle::TopLeft,
                        ResizeHandle::TopRight,
                        ResizeHandle::BottomLeft,
                        ResizeHandle::BottomRight,
                    ];
                    for handle in handles {
                        let c = handle_center(selection_rect, handle);
                        let actual_corner = handle_center(selection_rect_view, handle);
                        let is_clipped = actual_corner != c;
                        let r = egui::Rect::from_center_size(c, egui::vec2(handle_size, handle_size));
                        let fill = if is_clipped {
                            egui::Color32::from_rgba_unmultiplied(255, 174, 174, 240)
                        } else {
                            egui::Color32::from_white_alpha(230)
                        };
                        let stroke = if is_clipped {
                            egui::Color32::from_rgb(120, 72, 0)
                        } else {
                            egui::Color32::BLACK
                        };
                        painter.rect_filled(r, 0.0, fill);
                        painter.rect_stroke(r, 0.0, (1.0, stroke), egui::StrokeKind::Outside);
                    }
                }

                // Draw crosshair
                if app_state.is_show_crosshair {
                    if let Some(cursor_px) = app_state.cursor_pos {
                        // Draw a subtle shadow then a bright line for visibility
                        let painter = ui.painter();
                        let shadow = egui::Stroke {
                            width: 3.0,
                            color: egui::Color32::from_black_alpha(80),
                        };
                        let line = egui::Stroke {
                            width: 1.0,
                            color: egui::Color32::from_white_alpha(220),
                        };

                        for pane_rect in
                            std::iter::once(active_primary_rect).chain(split_view.then_some(right_pane_rect))
                        {
                            // Center of the hovered image pixel in view points
                            let center_px = egui::vec2(
                                (cursor_px.x as f32 + 0.5) * self.zoom(),
                                (cursor_px.y as f32 + 0.5) * self.zoom(),
                            );
                            let center_pt = pane_rect.min + (self.pan + center_px) / pixel_per_point;

                            let h1 = [
                                egui::pos2(pane_rect.left(), center_pt.y),
                                egui::pos2(pane_rect.right(), center_pt.y),
                            ];
                            let v1 = [
                                egui::pos2(center_pt.x, pane_rect.top()),
                                egui::pos2(center_pt.x, pane_rect.bottom()),
                            ];

                            painter.line_segment(h1, shadow);
                            painter.line_segment(v1, shadow);
                            painter.line_segment(h1, line);
                            painter.line_segment(v1, line);
                        }
                    }
                };

                // Draw per-pixel values when zoomed-in sufficiently and enabled
                if app_state.is_show_pixel_value && self.zoom() > 64.0 {
                    let painter = ui.painter();
                    // Determine font size relative to on-screen pixel size
                    let font_size = 16.0 / pixel_per_point;
                    let spacing = font_size * 0.1;
                    let font_id = egui::FontId::monospace(font_size);
                    let pane_iter = std::iter::once((active_primary_rect, primary_image))
                        .chain(split_view.then(|| (right_pane_rect, secondary_image.unwrap())));
                    for (pane_rect, pane_image) in pane_iter {
                        let top_left_img = self.pane_view_to_image_coords(pane_rect.min, pane_rect, pixel_per_point);
                        let bottom_right_img =
                            self.pane_view_to_image_coords(pane_rect.max, pane_rect, pixel_per_point);
                        let mut start_x = top_left_img.x.floor() as i32;
                        let mut start_y = top_left_img.y.floor() as i32;
                        let mut end_x = bottom_right_img.x.ceil() as i32;
                        let mut end_y = bottom_right_img.y.ceil() as i32;

                        start_x = start_x.max(0);
                        start_y = start_y.max(0);
                        end_x = end_x.min(spec.width);
                        end_y = end_y.min(spec.height);

                        for j in start_y..end_y {
                            for i in start_x..end_x {
                                if let Ok(vals) = pane_image.get_pixel_at(i, j) {
                                    let num_c = vals.len();
                                    let center_px =
                                        egui::vec2((i as f32 + 0.5) * self.zoom(), (j as f32 + 0.5) * self.zoom());
                                    let center_pt = pane_rect.min + (self.pan + center_px) / pixel_per_point;

                                    let total_h = (num_c as f32) * font_size;
                                    for (c_idx, v) in vals.iter().enumerate() {
                                        let y_offset = -total_h * 0.5 + (font_size + spacing) * (c_idx as f32 + 0.5);
                                        let pos = egui::pos2(center_pt.x, center_pt.y + y_offset);

                                        let color = match c_idx {
                                            0 => egui::Color32::RED,
                                            1 => egui::Color32::GREEN,
                                            2 => egui::Color32::BLUE,
                                            _ => egui::Color32::GRAY,
                                        };

                                        let text = if pane_image.spec().dtype.cv_type_is_floating() {
                                            format!("{:.4}", (*v as f64) * pane_image.spec().dtype.alpha())
                                        } else {
                                            format!("{:.0}", (*v as f64) * pane_image.spec().dtype.alpha())
                                        };
                                        painter.text(pos, egui::Align2::CENTER_CENTER, text, font_id.clone(), color);
                                    }
                                }
                            }
                        }
                    }
                }

                if split_view {
                    let divider_x = rect.center().x;
                    ui.painter().line_segment(
                        [egui::pos2(divider_x, rect.top()), egui::pos2(divider_x, rect.bottom())],
                        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(96)),
                    );
                }

                // Draw a continuous-position arrow on the viewport edge pointing toward the offscreen image
                // when the image is fully outside the viewport.
                if !split_view {
                    let scale = self.zoom();
                    let image_rect_view = {
                        let min_view = rect.min + (self.pan) / pixel_per_point;
                        let max_view = rect.min
                            + (self.pan + egui::vec2(spec.width as f32 * scale, spec.height as f32 * scale))
                                / pixel_per_point;
                        egui::Rect::from_min_max(min_view, max_view)
                    };

                    let fully_outside = image_rect_view.max.x < rect.min.x
                        || image_rect_view.min.x > rect.max.x
                        || image_rect_view.max.y < rect.min.y
                        || image_rect_view.min.y > rect.max.y;

                    if fully_outside {
                        let painter = ui.painter();
                        let view_bounds = rect;
                        let img_center = image_rect_view.center();
                        let view_center = view_bounds.center();
                        let dir = (img_center - view_center).normalized();
                        if dir.length_sq() > 0.0 {
                            // Find intersection of ray from center along dir with the viewport rectangle.
                            // Parameterize: p(t) = view_center + dir * t, find smallest t >= 0 where p is on an edge.
                            let mut t_candidates: Vec<f32> = Vec::with_capacity(4);
                            if dir.x.abs() > 1e-6 {
                                // Left edge x = view_bounds.min.x
                                let t = (view_bounds.min.x - view_center.x) / dir.x;
                                if t > 0.0 {
                                    let y = view_center.y + dir.y * t;
                                    if y >= view_bounds.min.y && y <= view_bounds.max.y {
                                        t_candidates.push(t);
                                    }
                                }
                                // Right edge
                                let t = (view_bounds.max.x - view_center.x) / dir.x;
                                if t > 0.0 {
                                    let y = view_center.y + dir.y * t;
                                    if y >= view_bounds.min.y && y <= view_bounds.max.y {
                                        t_candidates.push(t);
                                    }
                                }
                            }
                            if dir.y.abs() > 1e-6 {
                                // Top edge
                                let t = (view_bounds.min.y - view_center.y) / dir.y;
                                if t > 0.0 {
                                    let x = view_center.x + dir.x * t;
                                    if x >= view_bounds.min.x && x <= view_bounds.max.x {
                                        t_candidates.push(t);
                                    }
                                }
                                // Bottom edge
                                let t = (view_bounds.max.y - view_center.y) / dir.y;
                                if t > 0.0 {
                                    let x = view_center.x + dir.x * t;
                                    if x >= view_bounds.min.x && x <= view_bounds.max.x {
                                        t_candidates.push(t);
                                    }
                                }
                            }
                            if let Some(&t_edge) = t_candidates.iter().min_by(|a, b| a.partial_cmp(b).unwrap()) {
                                // Place arrow tip on an inset rectangle (shrunken viewport) instead of moving back along the ray.
                                let inset: f32 = 16.0;
                                let outer_tip = view_center + dir * t_edge; // actual intersection with outer edge
                                                                            // Build inset rectangle
                                let inset_rect = egui::Rect::from_min_max(
                                    egui::pos2(view_bounds.min.x + inset, view_bounds.min.y + inset),
                                    egui::pos2(view_bounds.max.x - inset, view_bounds.max.y - inset),
                                );
                                // Project a line from center to outer_tip; find intersection with inset_rect boundary.
                                // We reuse the parametric form; compute t for inset edges and choose smallest positive that lies on inset_rect perimeter.
                                let mut t_inset_candidates: Vec<f32> = Vec::with_capacity(4);
                                let vc = view_center;
                                if dir.x.abs() > 1e-6 {
                                    // left inset edge
                                    let t = (inset_rect.min.x - vc.x) / dir.x;
                                    if t > 0.0 {
                                        let y = vc.y + dir.y * t;
                                        if y >= inset_rect.min.y && y <= inset_rect.max.y {
                                            t_inset_candidates.push(t);
                                        }
                                    }
                                    // right inset edge
                                    let t = (inset_rect.max.x - vc.x) / dir.x;
                                    if t > 0.0 {
                                        let y = vc.y + dir.y * t;
                                        if y >= inset_rect.min.y && y <= inset_rect.max.y {
                                            t_inset_candidates.push(t);
                                        }
                                    }
                                }
                                if dir.y.abs() > 1e-6 {
                                    // top inset edge
                                    let t = (inset_rect.min.y - vc.y) / dir.y;
                                    if t > 0.0 {
                                        let x = vc.x + dir.x * t;
                                        if x >= inset_rect.min.x && x <= inset_rect.max.x {
                                            t_inset_candidates.push(t);
                                        }
                                    }
                                    // bottom inset edge
                                    let t = (inset_rect.max.y - vc.y) / dir.y;
                                    if t > 0.0 {
                                        let x = vc.x + dir.x * t;
                                        if x >= inset_rect.min.x && x <= inset_rect.max.x {
                                            t_inset_candidates.push(t);
                                        }
                                    }
                                }
                                let tip = if let Some(&t_inset) =
                                    t_inset_candidates.iter().min_by(|a, b| a.partial_cmp(b).unwrap())
                                {
                                    vc + dir * t_inset
                                } else {
                                    // Fallback: if something goes wrong, just put at outer intersection minus a tiny epsilon
                                    outer_tip - dir * 2.0
                                };

                                // Arrow dimensions
                                let shaft_len = 72.0; // total arrow length ~ shaft_len + head depth portion
                                let shaft_thickness = 8.0;
                                let head_len = 32.0;
                                let head_width = 32.0;
                                let stroke_col = egui::Color32::from_black_alpha(150);
                                let fill_col = egui::Color32::from_white_alpha(235);
                                let shadow_col = egui::Color32::from_black_alpha(40);

                                let d = dir;
                                let perp = egui::vec2(-d.y, d.x);

                                // Points: build arrow pointing in +d. We'll draw from base toward tip.
                                let total_len = shaft_len;
                                let base = tip - d * total_len;
                                let shaft_end = tip - d * head_len; // where head starts
                                let half_thickness = shaft_thickness * 0.5;
                                let half_head_w = head_width * 0.5;

                                // Shaft rectangle (4 points)
                                let s0 = base + perp * half_thickness;
                                let s1 = base - perp * half_thickness;
                                let s2 = shaft_end - perp * half_thickness;
                                let s3 = shaft_end + perp * half_thickness;

                                // Head triangle
                                let h0 = shaft_end + perp * half_head_w;
                                let h1 = shaft_end - perp * half_head_w;
                                let h2 = tip;

                                // Optional subtle shadow (draw first)
                                let shadow_offset = egui::vec2(1.0, 1.0);
                                let shadow_poly = vec![
                                    s0 + shadow_offset,
                                    s1 + shadow_offset,
                                    s2 + shadow_offset,
                                    s3 + shadow_offset,
                                ];
                                painter.add(egui::Shape::convex_polygon(shadow_poly, shadow_col, egui::Stroke::NONE));
                                painter.add(egui::Shape::convex_polygon(
                                    vec![h0 + shadow_offset, h1 + shadow_offset, h2 + shadow_offset],
                                    shadow_col,
                                    egui::Stroke::NONE,
                                ));

                                // Main shapes
                                let shaft = vec![s0, s1, s2, s3];
                                painter.add(egui::Shape::convex_polygon(
                                    shaft,
                                    fill_col,
                                    egui::Stroke {
                                        width: 1.0,
                                        color: stroke_col,
                                    },
                                ));
                                painter.add(egui::Shape::convex_polygon(
                                    vec![h0, h1, h2],
                                    fill_col,
                                    egui::Stroke {
                                        width: 1.0,
                                        color: stroke_col,
                                    },
                                ));
                            }
                        }
                    }
                }
            }
        } else {
            ui.colored_label(egui::Color32::RED, "No texture uploaded");
        }
    }

    pub fn reset_view(&mut self) {
        self.zoom_level = 0.0;
        self.pan = egui::Vec2::ZERO;
    }

    pub fn zoom(&self) -> f32 {
        self.zoom_base.powf(self.zoom_level)
    }

    pub fn zoom_in(&mut self, level: f32, center: Option<egui::Vec2>) {
        let old_scale = self.zoom();
        self.zoom_level += level;
        let new_scale = self.zoom();
        let k = new_scale / old_scale;

        let local = if let Some(center) = center {
            center
        } else {
            self.last_viewport_size_px.unwrap_or(vec2(0.0, 0.0)) * 0.5
        };

        self.pan = self.pan * k + local * (1.0 - k);
    }

    fn is_split_comparison(&self, app_state: &AppState) -> bool {
        app_state.is_comparison()
            && app_state.comparison_mode == crate::model::ComparisonMode::Split
            && app_state.asset_primary.is_some()
            && app_state.asset_secondary.is_some()
    }

    fn split_pane_rects(&self, rect: egui::Rect) -> (egui::Rect, egui::Rect) {
        let divider_x = rect.center().x;
        (
            egui::Rect::from_min_max(rect.min, egui::pos2(divider_x, rect.max.y)),
            egui::Rect::from_min_max(egui::pos2(divider_x, rect.min.y), rect.max),
        )
    }

    fn selection_rect_in_view(&self, pane_rect: egui::Rect, rect: Recti, pixel_per_point: f32) -> egui::Rect {
        let r = rect.to_rect();
        let k = self.zoom() / pixel_per_point;
        (r * k).translate(self.pan / pixel_per_point + pane_rect.min.to_vec2())
    }

    fn pane_view_to_image_coords(
        &self,
        view_pos: egui::Pos2,
        pane_rect: egui::Rect,
        pixel_per_point: f32,
    ) -> egui::Pos2 {
        let local_pos = (view_pos - pane_rect.min) * pixel_per_point;
        ((local_pos - self.pan) / self.zoom()).to_pos2()
    }

    pub fn view_to_image_coords(
        &self,
        view_pos: egui::Pos2,
        rect: egui::Rect,
        pixel_per_point: f32,
        split_view: bool,
    ) -> (egui::Pos2, bool) {
        if split_view {
            let (left_rect, right_rect) = self.split_pane_rects(rect);
            if right_rect.contains(view_pos) {
                return (self.pane_view_to_image_coords(view_pos, right_rect, pixel_per_point), true);
            }
            return (self.pane_view_to_image_coords(view_pos, left_rect, pixel_per_point), false);
        }

        (self.pane_view_to_image_coords(view_pos, rect, pixel_per_point), false)
    }

    fn resize_start_rect(
        &self,
        start_rect: Recti,
        handle: ResizeHandle,
        selection_rect_view: egui::Rect,
        selection_rect_clipped: egui::Rect,
        rect: egui::Rect,
        pixel_per_point: f32,
        split_view: bool,
    ) -> Recti {
        let actual_handle_pos = handle_center(selection_rect_view, handle);
        let visible_handle_pos = handle_center(selection_rect_clipped, handle);
        if actual_handle_pos == visible_handle_pos {
            return start_rect;
        }

        // If the handle is partially outside the viewport, we want to resize based on the visible position
        // which corresponds to the clipped selection rect.
        let visible_handle_image_pos = self
            .view_to_image_coords(visible_handle_pos, rect, pixel_per_point, split_view)
            .0;
        rect_with_handle_at(start_rect, handle, visible_handle_image_pos)
    }

    // Fit the given image-space rectangle fully within the last known viewport.
    // Chooses the largest integer zoom_level such that the rect is fully visible.
    pub fn fit_rect(&mut self, rect: Recti) {
        let Some(viewport_px) = self.last_viewport_size_px else {
            return;
        };
        let rw = rect.width().max(1) as f32;
        let rh = rect.height().max(1) as f32;
        let vw = viewport_px.x.max(1.0);
        let vh = viewport_px.y.max(1.0);

        // Maximum scale that still fits
        let scale_max = (vw / rw).min(vh / rh);
        if !scale_max.is_finite() || scale_max <= 0.0 {
            return;
        }

        // Compute integer zoom level n where zoom_base^n <= scale_max and n is maximal
        let base = self.zoom_base.max(1.0000001); // avoid ln(1)
        let n = (scale_max.ln() / base.ln()).floor();
        self.zoom_level = n;

        // Center the rect in the viewport
        let scale = self.zoom();
        let rect_cx = (rect.min.x as f32 + rect.max.x as f32) * 0.5;
        let rect_cy = (rect.min.y as f32 + rect.max.y as f32) * 0.5;
        let viewport_cx = vw * 0.5;
        let viewport_cy = vh * 0.5;
        self.pan = egui::vec2(viewport_cx - rect_cx * scale, viewport_cy - rect_cy * scale);
    }

    pub fn fit_split(&mut self, image_width: i32, image_height: i32) {
        let Some(viewport_px) = self.last_viewport_size_px else {
            return;
        };

        let rw = image_width.max(1) as f32;
        let rh = image_height.max(1) as f32;
        let vw = (viewport_px.x * 0.5).max(1.0);
        let vh = viewport_px.y.max(1.0);
        let scale_max = (vw / rw).min(vh / rh);
        if !scale_max.is_finite() || scale_max <= 0.0 {
            return;
        }

        let base = self.zoom_base.max(1.0000001);
        self.zoom_level = (scale_max.ln() / base.ln()).floor();

        let scale = self.zoom();
        let rect_cx = image_width as f32 * 0.5;
        let rect_cy = image_height as f32 * 0.5;
        let viewport_cx = vw * 0.5;
        let viewport_cy = vh * 0.5;
        self.pan = egui::vec2(viewport_cx - rect_cx * scale, viewport_cy - rect_cy * scale);
    }

    // Center the given image-space rectangle in the viewport without changing zoom.
    pub fn center_rect(&mut self, rect: Recti) {
        let Some(viewport_px) = self.last_viewport_size_px else {
            return;
        };
        let vw = viewport_px.x.max(1.0);
        let vh = viewport_px.y.max(1.0);
        let scale = self.zoom();
        let rect_cx = (rect.min.x as f32 + rect.max.x as f32) * 0.5;
        let rect_cy = (rect.min.y as f32 + rect.max.y as f32) * 0.5;
        let viewport_cx = vw * 0.5;
        let viewport_cy = vh * 0.5;
        self.pan = egui::vec2(viewport_cx - rect_cx * scale, viewport_cy - rect_cy * scale);
    }

    pub fn request_copy(&mut self, source_label: String) {
        self.copy_requested = Some(source_label);
    }

    pub fn request_save_dialog(&mut self) {
        self.save_dialog_requested = true;
    }

    pub fn take_save_dialog_request(&mut self) -> bool {
        std::mem::take(&mut self.save_dialog_requested)
    }

    pub fn request_save(&mut self, path: PathBuf, source_label: String) {
        self.save_requested = Some((path, source_label));
    }

    pub fn is_marquee_interaction_active(&self) -> bool {
        self.dragging && matches!(self.drag_mode, DragMode::Marquee { .. } | DragMode::Resizing { .. })
    }

    pub fn take_export_toasts(&mut self) -> Vec<(bool, String)> {
        let Ok(mut toasts) = self.export_toasts.lock() else {
            return Vec::new();
        };

        toasts
            .drain(..)
            .map(|toast| match toast {
                ExportToast::Success(message) => (true, message),
                ExportToast::Error(message) => (false, message),
            })
            .collect()
    }

    pub fn take_shader_error(&mut self) -> Option<String> {
        if self.last_shader_error.is_none() {
            self.last_reported_shader_error = None;
            return None;
        }
        if self.last_shader_error != self.last_reported_shader_error {
            self.last_reported_shader_error = self.last_shader_error.clone();
            return self.last_shader_error.clone();
        }
        None
    }

    fn refresh_shader_error(&mut self) {
        if let Some(image_prog) = &self.image_prog {
            if let Ok(program) = image_prog.lock() {
                self.last_shader_error = program.last_error().cloned();
            }
        }

        if self.last_shader_error.is_none() {
            self.last_reported_shader_error = None;
        }
    }

    fn copy_rect(&self, app_state: &AppState, image_width: i32, image_height: i32) -> Recti {
        let selection = app_state.marquee_rect.validate();
        if !selection.empty() {
            return selection;
        }

        Recti::from_min_size(vec2i(0, 0), vec2i(image_width, image_height))
    }
}

fn copy_image_to_clipboard(
    width: i32,
    height: i32,
    bytes: Vec<u8>,
    export_toasts: &Arc<Mutex<Vec<ExportToast>>>,
    source_label: &str,
) {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        let img = arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: std::borrow::Cow::Owned(bytes),
        };
        let copy_result = clipboard.set_image(img);
        if let Ok(mut toasts) = export_toasts.lock() {
            match copy_result {
                Ok(()) => {
                    let message = if source_label == "image" {
                        "Copied image to clipboard".to_string()
                    } else {
                        format!("Copied {source_label} image to clipboard")
                    };
                    toasts.push(ExportToast::Success(message))
                }
                Err(err) => {
                    eprintln!("Failed to copy image to clipboard: {err}");
                    let message = if source_label == "image" {
                        "Failed to copy image to clipboard".to_string()
                    } else {
                        format!("Failed to copy {source_label} image to clipboard")
                    };
                    toasts.push(ExportToast::Error(message));
                }
            }
        }
    } else if let Ok(mut toasts) = export_toasts.lock() {
        eprintln!("Failed to open clipboard for image copy");
        let message = if source_label == "image" {
            "Failed to copy image to clipboard".to_string()
        } else {
            format!("Failed to copy {source_label} image to clipboard")
        };
        toasts.push(ExportToast::Error(message));
    }
}

fn save_image_async(
    path: PathBuf,
    width: i32,
    height: i32,
    bytes: Vec<u8>,
    export_toasts: Arc<Mutex<Vec<ExportToast>>>,
    repaint_ctx: egui::Context,
    source_label: String,
) {
    std::thread::spawn(move || {
        let toast = match save_rendered_image(&path, width, height, bytes) {
            Ok(()) => {
                let message = if source_label == "image" {
                    format!("Saved image to {}", path.display())
                } else {
                    format!("Saved {source_label} image to {}", path.display())
                };
                ExportToast::Success(message)
            }
            Err(err) => {
                eprintln!("Failed to save image to {}: {err}", path.display());
                let message = if source_label == "image" {
                    format!("Failed to save image to {}", path.display())
                } else {
                    format!("Failed to save {source_label} image to {}", path.display())
                };
                ExportToast::Error(message)
            }
        };

        if let Ok(mut toasts) = export_toasts.lock() {
            toasts.push(toast);
        }
        repaint_ctx.request_repaint();
    });
}

fn save_rendered_image(path: &Path, width: i32, height: i32, rgba_bytes: Vec<u8>) -> Result<()> {
    if width <= 0 || height <= 0 {
        return Err(eyre!("Invalid export size: {}x{}", width, height));
    }

    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_else(|| "png".to_owned());

    let rgba_row = core::Mat::new_rows_cols_with_data(height, width * 4, &rgba_bytes)?;
    let rgba = rgba_row.reshape(4, height)?.clone_pointee();
    let mut encoded = core::Mat::default();
    let mut params = core::Vector::<i32>::new();
    let encode_ext = match extension.as_str() {
        "png" => ".png",
        "jpg" | "jpeg" => ".jpg",
        other => return Err(eyre!("Unsupported export extension: {other}")),
    };

    match extension.as_str() {
        "png" => {
            imgproc::cvt_color(
                &rgba,
                &mut encoded,
                imgproc::COLOR_RGBA2BGRA,
                0,
                core::AlgorithmHint::ALGO_HINT_DEFAULT,
            )?;
        }
        "jpg" | "jpeg" => {
            imgproc::cvt_color(
                &rgba,
                &mut encoded,
                imgproc::COLOR_RGBA2BGR,
                0,
                core::AlgorithmHint::ALGO_HINT_DEFAULT,
            )?;
            params.push(imgcodecs::IMWRITE_JPEG_QUALITY);
            params.push(95);
        }
        _ => unreachable!(),
    }

    let mut encoded_bytes = core::Vector::<u8>::new();
    if !imgcodecs::imencode(encode_ext, &encoded, &mut encoded_bytes, &params)? {
        return Err(eyre!("OpenCV failed to encode image as {encode_ext}"));
    }
    std::fs::write(path, encoded_bytes.as_slice())?;
    Ok(())
}

/// Keep a GL texture synchronized with the current image id so unchanged images do not trigger re-upload work.
fn sync_mat_texture(
    gl: &GL::Context,
    texture_slot: &mut Option<GL::NativeTexture>,
    last_image_id: &mut Option<u64>,
    image: &impl Image,
) {
    let image_id = image.id();
    if texture_slot.is_some() && Some(image_id) == *last_image_id {
        return;
    }

    if let Some(old_tex) = texture_slot.take() {
        unsafe {
            gl.delete_texture(old_tex);
        }
    }

    if let Ok(tex) = upload_mat_texture(gl, image) {
        *texture_slot = Some(tex);
        *last_image_id = Some(image_id);
    }
}

fn release_texture(gl: &GL::Context, texture_slot: &mut Option<GL::NativeTexture>, last_image_id: &mut Option<u64>) {
    if let Some(old_tex) = texture_slot.take() {
        unsafe {
            gl.delete_texture(old_tex);
        }
    }
    *last_image_id = None;
}

// Upload an CPU data directly as an OpenGL texture.
// Supports 1 (GRAY), 3 (BGR), 4 (BGRA) channel 8-bit mats.
fn upload_mat_texture(gl: &GL::Context, image: &impl Image) -> Result<GL::NativeTexture> {
    #[cfg(debug_assertions)]
    let _timer = crate::util::timer::ScopedTimer::new("Upload texture");

    let spec = image.spec();
    let (w, h) = (spec.width, spec.height);
    let channels = spec.channels;

    let total_elems = (w * h * channels) as usize;

    let bytes = image.data_ptr()?;
    let (head, f32s, tail) = unsafe { bytes.align_to::<f32>() };
    if !(head.is_empty() && tail.is_empty()) {
        return Err(eyre!("Data not properly aligned for f32"));
    }

    if f32s.len() != total_elems {
        return Err(eyre!("Unexpected data length: {} != {}", f32s.len(), total_elems));
    }

    let (internal, format) = match channels {
        1 => (GL::R32F, GL::RED),
        2 => (GL::RG32F, GL::RG),
        3 => (GL::RGB32F, GL::RGB),
        4 => (GL::RGBA32F, GL::RGBA),
        _ => return Err(eyre!("Unsupported channels: {channels}")),
    };

    unsafe {
        let tex = gl.create_texture().unwrap();
        gl.bind_texture(GL::TEXTURE_2D, Some(tex));
        gl.tex_parameter_i32(GL::TEXTURE_2D, GL::TEXTURE_MIN_FILTER, GL::LINEAR as _);
        gl.tex_parameter_i32(GL::TEXTURE_2D, GL::TEXTURE_MAG_FILTER, GL::NEAREST as _);
        gl.tex_parameter_i32(GL::TEXTURE_2D, GL::TEXTURE_WRAP_S, GL::CLAMP_TO_EDGE as _);
        gl.tex_parameter_i32(GL::TEXTURE_2D, GL::TEXTURE_WRAP_T, GL::CLAMP_TO_EDGE as _);
        gl.pixel_store_i32(GL::UNPACK_ALIGNMENT, 1);
        gl.tex_image_2d(
            GL::TEXTURE_2D,
            0,
            internal as i32,
            w,
            h,
            0,
            format,
            GL::FLOAT,
            GL::PixelUnpackData::Slice(Some(bytemuck::cast_slice(f32s))),
        );
        Ok(tex)
    }
}

fn hit_test_handles(selection_rect: egui::Rect, pointer: egui::Pos2) -> Option<ResizeHandle> {
    // Slightly larger hit area than the visual handle for easier grabbing.
    let handle_size = 16.0; // hit area in points
    let corners = [
        (handle_center(selection_rect, ResizeHandle::TopLeft), ResizeHandle::TopLeft),
        (handle_center(selection_rect, ResizeHandle::TopRight), ResizeHandle::TopRight),
        (
            handle_center(selection_rect, ResizeHandle::BottomLeft),
            ResizeHandle::BottomLeft,
        ),
        (
            handle_center(selection_rect, ResizeHandle::BottomRight),
            ResizeHandle::BottomRight,
        ),
    ];

    for (center, handle) in corners {
        let r = egui::Rect::from_center_size(center, egui::vec2(handle_size, handle_size));
        if r.contains(pointer) {
            return Some(handle);
        }
    }
    None
}

fn hit_test_resize_handle_with_rects(
    primary_selection_rect_view: egui::Rect,
    primary_selection_rect_clipped: egui::Rect,
    secondary_selection_rect_view: Option<egui::Rect>,
    secondary_selection_rect_clipped: Option<egui::Rect>,
    pointer: egui::Pos2,
) -> Option<(ResizeHandle, egui::Rect, egui::Rect)> {
    if let Some(handle) = hit_test_handles(primary_selection_rect_clipped, pointer) {
        return Some((handle, primary_selection_rect_view, primary_selection_rect_clipped));
    }

    secondary_selection_rect_view.zip(secondary_selection_rect_clipped).and_then(
        |(selection_rect_view, selection_rect_clipped)| {
            hit_test_handles(selection_rect_clipped, pointer)
                .map(|handle| (handle, selection_rect_view, selection_rect_clipped))
        },
    )
}

fn handle_center(selection_rect: egui::Rect, handle: ResizeHandle) -> egui::Pos2 {
    match handle {
        ResizeHandle::TopLeft => selection_rect.min,
        ResizeHandle::TopRight => egui::pos2(selection_rect.max.x, selection_rect.min.y),
        ResizeHandle::BottomLeft => egui::pos2(selection_rect.min.x, selection_rect.max.y),
        ResizeHandle::BottomRight => selection_rect.max,
    }
}

fn enforce_square_from_anchor(anchor: egui::Pos2, free: egui::Pos2) -> egui::Pos2 {
    let dx = free.x - anchor.x;
    let dy = free.y - anchor.y;
    let adx = dx.abs();
    let ady = dy.abs();
    if adx == 0.0 && ady == 0.0 {
        return free;
    }
    let side = adx.max(ady);
    let sx = if dx >= 0.0 { 1.0 } else { -1.0 };
    let sy = if dy >= 0.0 { 1.0 } else { -1.0 };
    egui::pos2(anchor.x + sx * side, anchor.y + sy * side)
}

fn rect_with_handle_at(rect: Recti, handle: ResizeHandle, handle_pos: egui::Pos2) -> Recti {
    // Clip the handle position to be within the rect bounds.
    let mut adjusted = rect;
    match handle {
        ResizeHandle::TopLeft => {
            adjusted.min.x = handle_pos.x.floor() as i32;
            adjusted.min.y = handle_pos.y.floor() as i32;
        }
        ResizeHandle::TopRight => {
            adjusted.max.x = handle_pos.x.ceil() as i32;
            adjusted.min.y = handle_pos.y.floor() as i32;
        }
        ResizeHandle::BottomLeft => {
            adjusted.min.x = handle_pos.x.floor() as i32;
            adjusted.max.y = handle_pos.y.ceil() as i32;
        }
        ResizeHandle::BottomRight => {
            adjusted.max.x = handle_pos.x.ceil() as i32;
            adjusted.max.y = handle_pos.y.ceil() as i32;
        }
    }
    adjusted.validate()
}
