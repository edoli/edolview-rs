use color_eyre::eyre::{eyre, Result};
use eframe::egui::{self, vec2};
use eframe::glow::{self as GL, HasContext};
use std::sync::{Arc, Mutex};

use crate::model::{AppState, Image, Recti, EMPTY_MINMAX};
use crate::ui::gl::{BackgroundProgram, ImageProgram};
use crate::util::func_ext::FuncExt;
use crate::util::math_ext::vec2i;

enum DragMode {
    None,
    Panning { last_pixel_pos: egui::Pos2 },
    Marquee { start_image_pos: egui::Pos2 },
    Resizing { handle: ResizeHandle, start_rect: Recti },
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
    gl_raw_tex: Option<GL::NativeTexture>,
    zoom_level: f32,
    zoom_base: f32,
    pan: egui::Vec2,
    dragging: bool,
    drag_mode: DragMode,
    copy_requested: bool,
    last_image_id: Option<u64>, // cache key to know when to re-upload texture
    last_viewport_size_px: Option<egui::Vec2>,
}

impl ImageViewer {
    pub fn new() -> Self {
        Self {
            background_prog: None,
            image_prog: None,
            gl_raw_tex: None,
            zoom_level: 0.0,
            zoom_base: 2.0_f32.powf(1.0 / 4.0),
            pan: egui::Vec2::ZERO,
            dragging: false,
            drag_mode: DragMode::None,
            copy_requested: false,
            last_image_id: None,
            last_viewport_size_px: None,
        }
    }

    pub fn show_image(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame, app_state: &mut AppState) {
        let Some(asset) = app_state.asset.as_ref() else {
            ui.centered_and_justified(|ui| {
                ui.label("Drag & Drop an image file here.");
            });
            return;
        };
        let image = asset.image();

        // Determine if we need a (re)upload
        let mut need_update_texture = false;
        let new_id = image.id();
        let spec = image.spec();
        let min_max = if app_state.shader_params.auto_minmax && !app_state.shader_params.use_per_channel
            || app_state.shader_params.auto_minmax_channels.iter().any(|&b| b)
        {
            image.minmax().clone()
        } else {
            EMPTY_MINMAX
        };

        if self.gl_raw_tex.is_none() || Some(new_id) != self.last_image_id {
            need_update_texture = true;
        }

        if need_update_texture {
            if let Some(gl) = frame.gl() {
                // Delete previous texture if any
                if let Some(old_tex) = self.gl_raw_tex.take() {
                    unsafe {
                        gl.delete_texture(old_tex);
                    }
                }
                if let Ok(tex) = upload_mat_texture(gl, image) {
                    self.gl_raw_tex = Some(tex);
                    self.last_image_id = Some(new_id);
                }
            }
        }

        if self.gl_raw_tex.is_some() {
            if self.background_prog.is_none() {
                if let Some(gl) = frame.gl() {
                    if let Ok(p) = BackgroundProgram::new(gl) {
                        self.background_prog = Some(Arc::new(p));
                    }
                }
            }

            if self.image_prog.is_none() {
                if let Some(gl) = frame.gl() {
                    if let Ok(p) = ImageProgram::new(gl) {
                        self.image_prog = Some(Arc::new(Mutex::new(p)));
                    }
                }
            }

            let available_points = ui.available_size();
            let (rect, resp) = ui.allocate_exact_size(available_points, egui::Sense::drag());
            let pixel_per_point = ui.ctx().pixels_per_point();
            let rect_pixels = rect * pixel_per_point;

            // Record viewport size in pixels for fit/center operations triggered from menus
            self.last_viewport_size_px = Some(vec2(rect_pixels.width(), rect_pixels.height()));

            // Pre-compute selection rect in view space (points) for handle interactions
            let selection_rect_view = {
                let r = app_state.marquee_rect.to_rect();
                let k = self.zoom() / pixel_per_point;
                (r * k).translate(self.pan / pixel_per_point + rect.min.to_vec2())
            };

            if resp.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll.abs() > 0.0 {
                    // Compute old scale before applying zoom change
                    let scroll_sign = scroll.signum();
                    if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                        let local = (pointer - rect.min) * pixel_per_point;
                        self.zoom_in(scroll_sign, Some(local));
                    }
                }

                let mouse_pos = ui.input(|i| i.pointer.hover_pos());
                if let Some(pointer_pos) = mouse_pos {
                    let local_pos = (pointer_pos - rect.min) * pixel_per_point;
                    let image_pos = (local_pos - self.pan) / self.zoom();
                    let pixel_pos = vec2i(image_pos.x as i32, image_pos.y as i32);
                    // Check if coordinates are within image bounds
                    if pixel_pos.x >= 0 && pixel_pos.x < spec.width && pixel_pos.y >= 0 && pixel_pos.y < spec.height {
                        app_state.cursor_pos = Some(pixel_pos);
                    } else {
                        app_state.cursor_pos = None;
                    }

                    // If marquee exists, set resize cursor when hovering corner handles
                    if app_state.marquee_rect.width() > 0 && app_state.marquee_rect.height() > 0 {
                        if let Some(handle) = hit_test_handles(selection_rect_view, pointer_pos) {
                            let icon = match handle {
                                ResizeHandle::TopLeft | ResizeHandle::BottomRight => egui::CursorIcon::ResizeNwSe,
                                ResizeHandle::TopRight | ResizeHandle::BottomLeft => egui::CursorIcon::ResizeNeSw,
                            };
                            ui.output_mut(|o| o.cursor_icon = icon);
                        }
                    }
                }
            }

            if resp.drag_started() {
                self.dragging = true;
                if let Some(pos) = resp.interact_pointer_pos() {
                    // If a marquee exists and a corner handle is grabbed, start resizing
                    let handle_under_mouse =
                        if app_state.marquee_rect.width() > 0 && app_state.marquee_rect.height() > 0 {
                            hit_test_handles(selection_rect_view, pos)
                        } else {
                            None
                        };

                    self.drag_mode = if let Some(handle) = handle_under_mouse {
                        DragMode::Resizing {
                            handle,
                            start_rect: app_state.marquee_rect,
                        }
                    } else if ui.input(|i| i.modifiers.shift) {
                        // Start marquee creation
                        DragMode::Marquee {
                            start_image_pos: self.view_to_image_coords(pos, rect, pixel_per_point),
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
                if let Some(pos) = resp.interact_pointer_pos() {
                    if let DragMode::Marquee { start_image_pos } = self.drag_mode {
                        // If Ctrl pressed, constrain to square relative to start
                        let is_ctrl = ui.input(|i| i.modifiers.ctrl);
                        let image_pos = self
                            .view_to_image_coords(pos, rect, pixel_per_point)
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
                    } else if let DragMode::Resizing { handle, start_rect } = self.drag_mode {
                        // Determine the fixed (opposite) corner from the start_rect
                        let (ax, ay) = match handle {
                            ResizeHandle::TopLeft => (start_rect.max.x as f32, start_rect.max.y as f32),
                            ResizeHandle::TopRight => (start_rect.min.x as f32, start_rect.max.y as f32),
                            ResizeHandle::BottomLeft => (start_rect.max.x as f32, start_rect.min.y as f32),
                            ResizeHandle::BottomRight => (start_rect.min.x as f32, start_rect.min.y as f32),
                        };
                        let anchor = egui::pos2(ax, ay);

                        // If Ctrl pressed, constrain to square relative to anchor
                        let is_ctrl = ui.input(|i| i.modifiers.ctrl);
                        let image_pos = self
                            .view_to_image_coords(pos, rect, pixel_per_point)
                            .cond_map(is_ctrl, |image_pos| enforce_square_from_anchor(anchor, image_pos));
                        app_state.set_marquee_rect(Recti::bound_two_pos(anchor, image_pos));
                    }
                }

                if resp.drag_stopped() {
                    self.dragging = false;
                    self.drag_mode = DragMode::None;
                }
            }

            // Recalculate selection rect in view space (points) for drawing
            let selection_rect_view = {
                let r = app_state.marquee_rect.to_rect();
                let k = self.zoom() / pixel_per_point;
                (r * k).translate(self.pan / pixel_per_point + rect.min.to_vec2())
            };

            // Queue a Ctrl+C copy operation to perform inside GL callback
            let mut copy_request: Option<(i32, i32, egui::Vec2, f32)> = None;
            if self.copy_requested {
                let sel = app_state.marquee_rect;
                if sel.width() > 0 && sel.height() > 0 {
                    let scale_for_copy = if app_state.copy_use_original_size {
                        1.0
                    } else {
                        self.zoom()
                    };
                    let out_w = (sel.width() as f32 * scale_for_copy).round().max(1.0) as i32;
                    let out_h = (sel.height() as f32 * scale_for_copy).round().max(1.0) as i32;
                    let position =
                        egui::vec2(-(sel.min.x as f32) * scale_for_copy, -(sel.min.y as f32) * scale_for_copy);
                    copy_request = Some((out_w, out_h, position, scale_for_copy));
                }
                self.copy_requested = false;
            }

            if let (Some(background_prog), Some(image_prog), Some(_gl)) =
                (self.background_prog.clone(), self.image_prog.clone(), frame.gl())
            {
                let viewport_size = vec2(rect_pixels.width() as f32, rect_pixels.height() as f32);
                let image_size = vec2(spec.width as f32, spec.height as f32);

                let tex_handle = self.gl_raw_tex.unwrap();
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

                ui.painter().add(egui::PaintCallback {
                    rect,
                    callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |info, painter| unsafe {
                        let gl = painter.gl();
                        let screen_h = info.screen_size_px[1] as i32;
                        let screen_w = info.screen_size_px[0] as i32;
                        let x = rect_pixels.min.x.round() as i32;
                        let y_top = rect_pixels.max.y.round() as i32;
                        let height = rect_pixels.height().round() as i32;
                        let y = screen_h - y_top;
                        let width = rect_pixels.width().round() as i32;
                        gl.viewport(x, y, width, height);

                        if is_show_background {
                            background_prog.draw(
                                gl,
                                viewport_size,
                                position,
                                16.0,
                                visuals.extreme_bg_color,
                                visuals.faint_bg_color,
                            );
                        }

                        if let Ok(mut image_prog) = image_prog.lock() {
                            image_prog.draw(
                                gl,
                                tex_handle,
                                colormap.as_str(),
                                viewport_size,
                                image_size,
                                channel_index,
                                &min_max,
                                is_mono,
                                scale,
                                position,
                                &shader_params,
                            );
                        }

                        // If a copy was requested, render to an offscreen FBO and place on clipboard
                        if let Some((out_w, out_h, crop_pos, copy_scale)) = copy_request {
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
                                    image_prog.draw(
                                        gl,
                                        tex_handle,
                                        colormap.as_str(),
                                        egui::vec2(out_w as f32, out_h as f32),
                                        image_size,
                                        channel_index,
                                        &min_max,
                                        is_mono,
                                        copy_scale,
                                        crop_pos,
                                        &shader_params,
                                    );
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

                                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                    let img = arboard::ImageData {
                                        width: out_w as usize,
                                        height: out_h as usize,
                                        bytes: std::borrow::Cow::Owned(flipped),
                                    };
                                    let _ = clipboard.set_image(img);
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
                let selection_rect = selection_rect_view.intersect(rect);
                if selection_rect.width() > 0.0 && selection_rect.height() > 0.0 {
                    ui.painter().rect_stroke(
                        selection_rect,
                        0.0,
                        (1.0, egui::Color32::from_gray(150)),
                        egui::StrokeKind::Middle,
                    );

                    // Draw corner handles (small squares)
                    let painter = ui.painter();
                    let handle_size = 8.0; // in points
                    let corners = [
                        selection_rect.min,                                     // TL
                        egui::pos2(selection_rect.max.x, selection_rect.min.y), // TR
                        egui::pos2(selection_rect.min.x, selection_rect.max.y), // BL
                        selection_rect.max,                                     // BR
                    ];
                    for &c in &corners {
                        let r = egui::Rect::from_center_size(c, egui::vec2(handle_size, handle_size));
                        painter.rect_filled(r, 0.0, egui::Color32::from_white_alpha(230));
                        painter.rect_stroke(r, 0.0, (1.0, egui::Color32::BLACK), egui::StrokeKind::Outside);
                    }
                }

                // Draw crosshair
                if app_state.is_show_crosshair {
                    if let Some(cursor_px) = app_state.cursor_pos {
                        // Center of the hovered image pixel in view points
                        let center_px = egui::vec2(
                            (cursor_px.x as f32 + 0.5) * self.zoom(),
                            (cursor_px.y as f32 + 0.5) * self.zoom(),
                        );
                        let center_pt = rect.min + (self.pan + center_px) / pixel_per_point;

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

                        let h1 = [
                            egui::pos2(rect.left(), center_pt.y),
                            egui::pos2(rect.right(), center_pt.y),
                        ];
                        let v1 = [
                            egui::pos2(center_pt.x, rect.top()),
                            egui::pos2(center_pt.x, rect.bottom()),
                        ];

                        painter.line_segment(h1, shadow);
                        painter.line_segment(v1, shadow);
                        painter.line_segment(h1, line);
                        painter.line_segment(v1, line);
                    }
                };

                // Draw per-pixel values when zoomed-in sufficiently and enabled
                if app_state.is_show_pixel_value && self.zoom() > 64.0 {
                    let painter = ui.painter();

                    // Compute visible image coordinate bounds
                    let top_left_img = self.view_to_image_coords(rect.min, rect, pixel_per_point);
                    let bottom_right_img = self.view_to_image_coords(rect.max, rect, pixel_per_point);

                    let mut start_x = top_left_img.x.floor() as i32;
                    let mut start_y = top_left_img.y.floor() as i32;
                    let mut end_x = bottom_right_img.x.ceil() as i32;
                    let mut end_y = bottom_right_img.y.ceil() as i32;

                    // Clamp to image bounds
                    start_x = start_x.max(0);
                    start_y = start_y.max(0);
                    end_x = end_x.min(spec.width);
                    end_y = end_y.min(spec.height);

                    // Determine font size relative to on-screen pixel size
                    let font_size = 16.0 / pixel_per_point;
                    let spacing = font_size * 0.1;
                    let font_id = egui::FontId::monospace(font_size);

                    if let Some(asset) = app_state.asset.as_ref() {
                        let image = asset.image();
                        for j in start_y..end_y {
                            for i in start_x..end_x {
                                if let Ok(vals) = image.get_pixel_at(i, j) {
                                    let num_c = vals.len();

                                    // Center of the image pixel in points
                                    let center_px =
                                        egui::vec2((i as f32 + 0.5) * self.zoom(), (j as f32 + 0.5) * self.zoom());
                                    let center_pt = rect.min + (self.pan + center_px) / pixel_per_point;

                                    // Arrange channel lines vertically centered within the pixel cell
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

                                        let text = format!("{:.4}", v);
                                        painter.text(pos, egui::Align2::CENTER_CENTER, text, font_id.clone(), color);
                                    }
                                }
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

    pub fn view_to_image_coords(&self, view_pos: egui::Pos2, rect: egui::Rect, pixel_per_point: f32) -> egui::Pos2 {
        let local_pos = (view_pos - rect.min) * pixel_per_point;
        ((local_pos - self.pan) / self.zoom()).to_pos2()
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

    pub fn request_copy(&mut self) {
        self.copy_requested = true;
    }
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
    let handle_size = 10.0; // hit area in points
    let corners = [
        (selection_rect.min, ResizeHandle::TopLeft),
        (egui::pos2(selection_rect.max.x, selection_rect.min.y), ResizeHandle::TopRight),
        (egui::pos2(selection_rect.min.x, selection_rect.max.y), ResizeHandle::BottomLeft),
        (selection_rect.max, ResizeHandle::BottomRight),
    ];

    for (center, handle) in corners {
        let r = egui::Rect::from_center_size(center, egui::vec2(handle_size, handle_size));
        if r.contains(pointer) {
            return Some(handle);
        }
    }
    None
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
