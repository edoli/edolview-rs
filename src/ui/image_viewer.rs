use std::sync::Arc;
use eframe::{egui};
use eframe::glow::{self, HasContext};
use opencv::core;
use opencv::prelude::*;
use color_eyre::eyre::{Result, eyre};

use crate::{
	ui::gl::ImageProgram,
	model::ImageSpec,
};

pub struct ImageViewer {
	gl_prog: Option<Arc<ImageProgram>>,
	gl_raw_tex: Option<glow::NativeTexture>,
	image_spec: Option<ImageSpec>,
	zoom_level: f32,
	zoom_base: f32,
	pan: egui::Vec2,
	dragging: bool,
	last_drag_pos: egui::Pos2,
}

impl ImageViewer {
	pub fn new() -> Self {
		Self {
			gl_prog: None,
			gl_raw_tex: None,
			image_spec: None,
			zoom_level: 0.0,
			zoom_base: 1.1,
			pan: egui::Vec2::ZERO,
			dragging: false,
			last_drag_pos: egui::Pos2::ZERO,
		}
	}
	
	pub fn show_image(
		&mut self,
		ui: &mut egui::Ui,
		frame: &mut eframe::Frame,
		mat: &core::Mat,
		grayscale: bool,
	) {
		// Upload texture if needed
		if self.gl_raw_tex.is_none() {
			if let Some(gl) = frame.gl() {

				self.image_spec = Some(ImageSpec::new(mat));

				if let Ok(ci) = mat_to_color_image(mat) {
					let size = [ci.width() as i32, ci.height() as i32];
					unsafe {
						let tex = gl.create_texture().unwrap();
						gl.bind_texture(glow::TEXTURE_2D, Some(tex));
						gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
						gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
						gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
						gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
						gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
						gl.tex_image_2d(
							glow::TEXTURE_2D, 0, glow::RGBA8 as i32, size[0], size[1], 0,
							glow::RGBA, glow::UNSIGNED_BYTE,
							glow::PixelUnpackData::Slice(Some(bytemuck::cast_slice(&ci.pixels)))
						);
						self.gl_raw_tex = Some(tex);
					}
				}
			}
		}

		if self.gl_raw_tex.is_some() {
			if self.gl_prog.is_none() {
				if let Some(gl) = frame.gl() {
					if let Ok(p) = ImageProgram::new(gl) { self.gl_prog = Some(Arc::new(p)); }
				}
			}
			let available_points = ui.available_size();
			let (rect, resp) = ui.allocate_exact_size(available_points, egui::Sense::drag());
			let rect_pixels = rect * ui.ctx().pixels_per_point();

			if resp.hovered() {
				let scroll = ui.input(|i| i.raw_scroll_delta.y);
				if scroll.abs() > 0.0 {
					// Compute old scale before applying zoom change
					let old_scale = self.zoom();
					let scroll_sign = scroll.signum();
					self.zoom_level += scroll_sign; // update zoom level first
					let new_scale = self.zoom();
					let k = new_scale / old_scale; // scaling factor applied to existing pan
					if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
						let local = pointer - rect.center();
						let clip = egui::Vec2::new(local.x / (rect.width()/2.0), local.y / (rect.height()/2.0));

						self.pan.x = self.pan.x * k + clip.x * (1.0 - k);
						self.pan.y = self.pan.y * k + clip.y * (1.0 - k);
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
					let dx = delta.x / (rect.width()/2.0);
					let dy = delta.y / (rect.height()/2.0);
					self.pan += egui::vec2(dx, dy);
				}
				if resp.drag_stopped() { 
					self.dragging = false;
				}
			}

			if let (Some(gl_prog), Some(_gl)) = (self.gl_prog.clone(), frame.gl()) {
				let tex_handle = self.gl_raw_tex.unwrap();
				let scale = self.zoom() as f32;
				let offset = self.pan;
				let grayscale_flag = grayscale;
				// base image scale: when scale==1.0 image shows 1:1 pixels
				let mut img_scale = (1.0_f32, 1.0_f32);
				if let Some(spec) = &self.image_spec {
					let viewport_w_px = rect_pixels.width() as f32;
					let viewport_h_px = rect_pixels.height() as f32;
					if viewport_w_px > 0.0 && viewport_h_px > 0.0 {
						img_scale = (spec.width as f32 / viewport_w_px, spec.height as f32 / viewport_h_px);
					}
				}
				ui.painter().add(egui::PaintCallback {
					rect,
					callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |info, painter| {
						unsafe {
							let gl = painter.gl();
							let screen_h = info.screen_size_px[1] as i32;
							let screen_w = info.screen_size_px[0] as i32;
							let x = rect_pixels.min.x.round() as i32;
							let y_top = rect_pixels.max.y.round() as i32;
							let height = rect_pixels.height().round() as i32;
							let y = screen_h - y_top;
							let width = rect_pixels.width().round() as i32;
							gl.viewport(x, y, width, height);
							gl_prog.draw(gl, tex_handle, grayscale_flag, scale, (offset.x, -offset.y), img_scale);
							gl.viewport(0, 0, screen_w, screen_h);
						}
					}) ),
				});
			}
		} else {
			ui.colored_label(egui::Color32::RED, "No texture uploaded");
		}
	}

	pub fn reset_view(&mut self) {
		// zoom_level baseline 0 => powf -> 1.0
		self.zoom_level = 0.0;
		self.pan = egui::Vec2::ZERO;
	}

    pub fn zoom(&self) -> f32 {
        self.zoom_base.powf(self.zoom_level)
    }
}

fn mat_to_color_image(mat: &core::Mat) -> Result<egui::ColorImage> {
	let size = mat.size()?;
	let (w, h) = (size.width as usize, size.height as usize);
	let channels = mat.channels();
	let buf = mat.data_bytes()?.to_vec();
	let mut rgba = Vec::with_capacity(w * h * 4);
	match channels {
		1 => { for &v in &buf { rgba.extend_from_slice(&[v, v, v, 255]); } }
		3 => { for chunk in buf.chunks_exact(3) { rgba.extend_from_slice(&[chunk[2], chunk[1], chunk[0], 255]); } }
		4 => { for chunk in buf.chunks_exact(4) { rgba.extend_from_slice(&[chunk[2], chunk[1], chunk[0], chunk[3]]); } }
	_ => return Err(eyre!("Unsupported channel count: {}", channels)),
	}
	Ok(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}
