use color_eyre::eyre::{eyre, Result};
use eframe::egui::{self, vec2};
use eframe::glow::{self as GL, HasContext};
use std::sync::{Arc, Mutex};

use crate::model::{AppState, Image, Recti, EMPTY_MINMAX};
use crate::res::KeyboardShortcutExt;
use crate::ui::gl::{BackgroundProgram, ImageProgram};
use crate::util::cv_ext::CvIntExt;
use crate::util::func_ext::FuncExt;
use crate::util::math_ext::vec2i;

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
            // Enable both drag (for panning / marquee) and click (for context menu)
            let (rect, resp) = ui.allocate_exact_size(available_points, egui::Sense::click_and_drag());
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

            // Detect if the built-in context menu popup for this response is open
            let context_menu_open = resp.context_menu_opened();

            if resp.hovered() && !context_menu_open {
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

            // Context menu (right-click)
            resp.context_menu(|ui| {
                if ui
                    .button(format!("Copy Selected Image ({})", crate::res::COPY_SC.format_sys()))
                    .clicked()
                {
                    self.request_copy();
                    ui.close();
                }
                if ui.button("Copy Cursor Color").clicked() {
                    if let Some(cursor_pos) = app_state.cursor_pos {
                        if let Ok(vals) = image.get_pixel_at(cursor_pos.x, cursor_pos.y) {
                            let spec = image.spec();
                            let alpha = spec.dtype.alpha();
                            let is_float = spec.dtype.cv_type_is_floating();
                            let mut parts: Vec<String> = Vec::with_capacity(vals.len());
                            for v in vals.iter() {
                                if is_float {
                                    parts.push(format!("{:.4}", (*v as f64) * alpha));
                                } else {
                                    parts.push(format!("{:.0}", (*v as f64) * alpha));
                                }
                            }
                            let text = parts.join(", ");
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(text);
                            }
                        }
                    }
                    ui.close();
                }
                if ui.button("Copy Cursor").clicked() {
                    if let Some(cursor_pos) = app_state.cursor_pos {
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            let _ = cb.set_text(format!("{}, {}", cursor_pos.x, cursor_pos.y));
                        }
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
                            hit_test_handles(selection_rect_view, pos)
                        } else {
                            None
                        };

                    if let Some(handle) = handle_under_mouse {
                        self.dragging = true;
                        self.drag_mode = DragMode::Resizing {
                            handle,
                            start_rect: app_state.marquee_rect,
                            start_pointer_image_pos: self.view_to_image_coords(pos, rect, pixel_per_point),
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
                            hit_test_handles(selection_rect_view, pos)
                        } else {
                            None
                        };

                    self.drag_mode = if let Some(handle) = handle_under_mouse {
                        DragMode::Resizing {
                            handle,
                            start_rect: app_state.marquee_rect,
                            start_pointer_image_pos: self.view_to_image_coords(pos, rect, pixel_per_point),
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
                let pos_opt = resp.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos()));
                if let Some(pos) = pos_opt {
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
                    } else if let DragMode::Resizing {
                        handle,
                        start_rect,
                        start_pointer_image_pos,
                    } = self.drag_mode
                    {
                        // Compute delta in image space from the initial press
                        let curr_img = self.view_to_image_coords(pos, rect, pixel_per_point);
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

                                        let text = if spec.dtype.cv_type_is_floating() {
                                            format!("{:.4}", (*v as f64) * spec.dtype.alpha())
                                        } else {
                                            format!("{:.0}", (*v as f64) * spec.dtype.alpha())
                                        };
                                        painter.text(pos, egui::Align2::CENTER_CENTER, text, font_id.clone(), color);
                                    }
                                }
                            }
                        }
                    }
                }

                // Draw a continuous-position arrow on the viewport edge pointing toward the offscreen image
                // when the image is fully outside the viewport.
                {
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
    // Slightly larger hit area than the visual handle for easier grabbing.
    let handle_size = 16.0; // hit area in points
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
