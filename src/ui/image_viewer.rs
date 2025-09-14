use color_eyre::eyre::{eyre, Result};
use eframe::egui;
use eframe::glow::{self as GL, HasContext};
use std::sync::Arc;

use crate::model::{DataType, Image};
use crate::ui::gl::{ImageProgram, ShaderParams};

pub struct ImageViewer {
    gl_prog: Option<Arc<ImageProgram>>,
    gl_raw_tex: Option<GL::NativeTexture>,
    zoom_level: f32,
    zoom_base: f32,
    pan: egui::Vec2,
    dragging: bool,
    last_drag_pos: egui::Pos2,
    last_image_id: Option<u64>, // cache key to know when to re-upload texture
}

impl ImageViewer {
    pub fn new() -> Self {
        Self {
            gl_prog: None,
            gl_raw_tex: None,
            zoom_level: 0.0,
            zoom_base: 2.0_f32.powf(1.0 / 4.0),
            pan: egui::Vec2::ZERO,
            dragging: false,
            last_drag_pos: egui::Pos2::ZERO,
            last_image_id: None,
        }
    }

    pub fn show_image(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame, image: &impl Image) {
        // Determine if we need a (re)upload
        let mut need_update_texture = false;
        let new_id = image.id();

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
            if self.gl_prog.is_none() {
                if let Some(gl) = frame.gl() {
                    if let Ok(p) = ImageProgram::new(gl) {
                        self.gl_prog = Some(Arc::new(p));
                    }
                }
            }

            let available_points = ui.available_size();
            let (rect, resp) = ui.allocate_exact_size(available_points, egui::Sense::drag());
            let pixel_per_point = ui.ctx().pixels_per_point();
            let rect_pixels = rect * pixel_per_point;

            if resp.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll.abs() > 0.0 {
                    // Compute old scale before applying zoom change
                    let old_scale = self.zoom();
                    let scroll_sign = scroll.signum();
                    self.zoom_level += scroll_sign;
                    let new_scale = self.zoom();
                    let k = new_scale / old_scale;
                    if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                        let local = (pointer - rect.min) * pixel_per_point;
                        self.pan = self.pan * k + local * (1.0 - k);
                    }
                }
            }

            if resp.drag_started() {
                self.dragging = true;
                self.last_drag_pos = resp.interact_pointer_pos().unwrap_or(self.last_drag_pos);
            }

            if self.dragging {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let delta = pos - self.last_drag_pos;
                    self.last_drag_pos = pos;
                    let dx = delta.x * pixel_per_point;
                    let dy = delta.y * pixel_per_point;
                    self.pan += egui::vec2(dx, dy);
                }
                if resp.drag_stopped() {
                    self.dragging = false;
                }
            }

            if let (Some(gl_prog), Some(_gl)) = (self.gl_prog.clone(), frame.gl()) {
                let viewport_w_px = rect_pixels.width() as f32;
                let viewport_h_px = rect_pixels.height() as f32;

                let tex_handle = self.gl_raw_tex.unwrap();
                let scale = self.zoom() as f32;
                let position = self.pan / egui::vec2(viewport_w_px, viewport_h_px) * 2.0;
                let spec = image.spec();

                let mut pixel_scale = egui::vec2(1.0, 1.0);
                if viewport_w_px > 0.0 && viewport_h_px > 0.0 {
                    // image will be strectched when pixel_scale is 1.0.
                    // To show 1:1 pixels, set pixel_scale to (spec.width/viewport_w, spec.height/viewport_h)
                    pixel_scale = egui::vec2(spec.width as f32 / viewport_w_px, spec.height as f32 / viewport_h_px);
                }
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
                        let shader_params = ShaderParams {
                            offset: 0.0,
                            exposure: 0.0,
                            gamma: 1.0,
                            min_v: 0.0,
                            max_v: 1.0,
                            r_scale: 1.0,
                            g_scale: 1.0,
                            b_scale: 1.0,
                            is_reciprocal: false,
                        };
                        gl_prog.draw(gl, tex_handle, scale, position, pixel_scale, shader_params);
                        gl.viewport(0, 0, screen_w, screen_h);
                    })),
                });
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
}

// Upload an CPU data directly as an OpenGL texture.
// Supports 1 (GRAY), 3 (BGR), 4 (BGRA) channel 8-bit mats.
fn upload_mat_texture(gl: &GL::Context, image: &impl Image) -> Result<GL::NativeTexture> {
    let spec = image.spec();
    let (w, h) = (spec.width, spec.height);
    let channels = spec.channels;
    if spec.dtype != f32::typ() {
        return Err(eyre!("Expected f32 data type, got {}", spec.dtype));
    }

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
