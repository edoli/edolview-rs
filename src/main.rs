use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use eframe::{egui, egui::ColorImage};
use std::sync::Arc;
use eframe::glow::{self, HasContext};
mod shader;
use shader::ImageProgram;
use opencv::{core, imgcodecs, imgproc, prelude::*};
use std::path::PathBuf;

/// Simple image viewer using OpenCV for decoding + egui for GUI.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about=None)]
struct Args {
    /// Path to the image file to open
    image: PathBuf,

    /// Window title
    #[arg(short, long, default_value_t=String::from("edolview"))]
    title: String,

    /// Max initial window size (WIDTHxHEIGHT). 0x0 keeps original size.
    #[arg(short='s', long="max-size", default_value="0x0")]
    max_size: String,

    /// Start in grayscale
    #[arg(short='g', long)]
    grayscale: bool,
}

fn parse_max_size(spec: &str) -> Result<(i32, i32)> {
    if spec == "0x0" {
        return Ok((0, 0));
    }
    let parts: Vec<_> = spec.split('x').collect();
    if parts.len() != 2 {
        return Err(eyre!("Invalid size format. Use WIDTHxHEIGHT"));
    }
    let w: i32 = parts[0].parse()?;
    let h: i32 = parts[1].parse()?;
    Ok((w, h))
}

struct ImageState {
    path: PathBuf,
    original: core::Mat,
    display: core::Mat,
    texture: Option<egui::TextureHandle>,
    grayscale: bool,
    max_w: i32,
    max_h: i32,
}

impl ImageState {
    fn load(path: PathBuf, grayscale: bool, max_w: i32, max_h: i32) -> Result<Self> {
        if !path.exists() {
            return Err(eyre!("Image does not exist: {:?}", path));
        }
        let img = imgcodecs::imread(path.to_string_lossy().as_ref(), imgcodecs::IMREAD_UNCHANGED)?;
        if img.empty() {
            return Err(eyre!("Failed to load image"));
        }
    let display = img.clone();
        let mut state = ImageState {
            path,
            original: img,
            display: core::Mat::default(),
            texture: None,
            grayscale,
            max_w,
            max_h,
        };
        state.display = state.process(&display)?;
        Ok(state)
    }

    fn process(&self, img: &core::Mat) -> Result<core::Mat> {
    let mut out = img.clone();
    // If alpha, drop alpha for display for now.
    if out.channels() == 4 {
            let mut bgr = core::Mat::default();
            imgproc::cvt_color(img, &mut bgr, imgproc::COLOR_BGRA2BGR, 0, core::AlgorithmHint::ALGO_HINT_DEFAULT)?;
            out = bgr;
        }
    if self.grayscale {
            let mut gray = core::Mat::default();
            imgproc::cvt_color(&out, &mut gray, imgproc::COLOR_BGR2GRAY, 0, core::AlgorithmHint::ALGO_HINT_DEFAULT)?;
            out = gray;
        }

        if self.max_w > 0 && self.max_h > 0 {
            let size = out.size()?;
            let (w, h) = (size.width, size.height);
            if w > self.max_w || h > self.max_h {
                let scale = f64::min(self.max_w as f64 / w as f64, self.max_h as f64 / h as f64);
                let new_w = (w as f64 * scale).round() as i32;
                let new_h = (h as f64 * scale).round() as i32;
                let mut resized = core::Mat::default();
                imgproc::resize(&out, &mut resized, core::Size::new(new_w, new_h), 0.0, 0.0, imgproc::INTER_LINEAR)?;
                out = resized;
            }
        }
        Ok(out)
    }

    fn rebuild(&mut self) -> Result<()> {
        self.display = self.process(&self.original.clone())?;
        // Texture will be recreated lazily next frame
        self.texture = None;
        Ok(())
    }

    fn to_color_image(&self) -> Result<ColorImage> {
        let mat = &self.display;
        let size = mat.size()?;
        let (w, h) = (size.width as usize, size.height as usize);
    let channels = mat.channels();
    let buf = mat.data_bytes()?.to_vec();
        // Convert BGR / Gray -> RGBA8 for egui
        let mut rgba = Vec::with_capacity(w * h * 4);
        match channels {
            1 => {
                for &v in &buf { rgba.extend_from_slice(&[v, v, v, 255]); }
            }
            3 => {
                for chunk in buf.chunks_exact(3) { rgba.extend_from_slice(&[chunk[2], chunk[1], chunk[0], 255]); }
            }
            4 => {
                for chunk in buf.chunks_exact(4) { rgba.extend_from_slice(&[chunk[2], chunk[1], chunk[0], chunk[3]]); }
            }
            _ => return Err(eyre!("Unsupported channel count: {}", channels)),
        }
        Ok(ColorImage::from_rgba_unmultiplied([w, h], &rgba))
    }
}

struct ViewerApp {
    state: ImageState,
    gl_prog: Option<Arc<ImageProgram>>,
    gl_raw_tex: Option<glow::NativeTexture>,
    // Interaction
    zoom: f32,          // isotropic scale in clip space (1.0 = fit panel)
    pan: egui::Vec2,    // clip-space offset
    dragging: bool,
    last_drag_pos: egui::Pos2,
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button(if self.state.grayscale { "Grayscale ✔" } else { "Grayscale" }).clicked() {
                    self.state.grayscale = !self.state.grayscale;
                    if let Err(e) = self.state.rebuild() { eprintln!("Error rebuilding image: {e}"); }
                    self.gl_raw_tex = None; // Force re-upload
                }
                if ui.button("Reload").clicked() {
                    if let Err(e) = self.state.rebuild() { eprintln!("Error reloading: {e}"); }
                    self.gl_raw_tex = None;
                }
                if ui.button("Reset View").clicked() {
                    self.zoom = 1.0;
                    self.pan = egui::Vec2::ZERO;
                }
                ui.label(format!("{}", self.state.path.display()));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // Prepare egui texture from Mat (we upload raw & let shader sample it)
            if self.gl_raw_tex.is_none() {
                if let Some(gl) = frame.gl() {
                    if let Ok(ci) = self.state.to_color_image() {
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
                                glow::TEXTURE_2D,
                                0,
                                glow::RGBA8 as i32,
                                size[0],
                                size[1],
                                0,
                                glow::RGBA,
                                glow::UNSIGNED_BYTE,
                                Some(bytemuck::cast_slice(&ci.pixels)),
                            );
                            self.gl_raw_tex = Some(tex);
                        }
                    }
                }
            }
            if self.gl_raw_tex.is_some() {
                // Ensure shader program exists
                if self.gl_prog.is_none() {
                    // Access glow context
                    if let Some(gl) = frame.gl() {
                        match ImageProgram::new(gl) {
                            Ok(p) => self.gl_prog = Some(Arc::new(p)),
                            Err(e) => eprintln!("Shader program error: {e}"),
                        }
                    }
                }
                let desired = ui.available_size();
                let (rect, resp) = ui.allocate_exact_size(desired, egui::Sense::drag());

                // Handle scroll for zoom (center on mouse)
                if resp.hovered() {
                    let scroll = ui.input(|i| i.raw_scroll_delta.y);
                    if scroll.abs() > 0.0 {
                        // scale factor per notch
                        let factor = (1.0 + scroll * 0.1).clamp(0.1, 10.0);
                        // Convert mouse pos to clip space before zoom to keep point under cursor
                        if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                            let center = rect.center();
                            let rel = pointer - center; // in ui points
                            // Map to clip: [-1,1] range assuming rect fully covers clip quad
                            let rel_clip = egui::Vec2::new(rel.x / (rect.width()/2.0), rel.y / (rect.height()/2.0));
                            // Adjust pan so that rel point stays stable
                            self.pan -= rel_clip * (factor - 1.0) * self.zoom;
                        }
                        self.zoom = (self.zoom * factor).clamp(0.05, 100.0);
                    }
                }

                // Drag for panning
                if resp.drag_started() { self.dragging = true; self.last_drag_pos = resp.interact_pointer_pos().unwrap_or(self.last_drag_pos); }
                if self.dragging {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        let delta = pos - self.last_drag_pos;
                        self.last_drag_pos = pos;
                        // Convert delta in ui points to clip space offset
                        let dx = delta.x / (rect.width()/2.0);
                        let dy = delta.y / (rect.height()/2.0);
                        self.pan += egui::vec2(dx, dy);
                    }
                    if resp.drag_released() { self.dragging = false; }
                }

                if let (Some(gl_prog), Some(gl)) = (self.gl_prog.clone(), frame.gl()) {
                    // Register a custom paint callback
                    let grayscale = self.state.grayscale;
                    let tex_handle = self.gl_raw_tex.unwrap();
                    let scale = self.zoom;
                    let offset = self.pan;
                    ui.painter().add(egui::PaintCallback {
                        rect,
                        callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |info, painter| {
                            unsafe {
                                let gl = painter.gl();
                                let screen_h = info.screen_size_px[1] as i32;
                                let screen_w = info.screen_size_px[0] as i32;
                                let x = rect.min.x.round() as i32;
                                let y_top = rect.max.y.round() as i32; // egui origin top-left
                                let height = rect.height().round() as i32;
                                let y = screen_h - y_top; // convert to GL origin bottom-left
                                let width = rect.width().round() as i32;
                                gl.viewport(x, y, width, height);
                                // Don't clear or disable scissor: that would wipe earlier drawn egui UI.
                                gl_prog.draw(gl, tex_handle, grayscale, scale, (offset.x, -offset.y)); // invert y for GL
                                // Restore viewport so subsequent egui draws (e.g. text glyphs) use full surface.
                                gl.viewport(0, 0, screen_w, screen_h);
                            }
                        })),
                    });
                } else {
                    ui.colored_label(egui::Color32::RED, "GL context not ready");
                }
            }
        });
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let (max_w, max_h) = parse_max_size(&args.max_size)?;
    let state = ImageState::load(args.image.clone(), args.grayscale, max_w, max_h)?;
    let native_options = eframe::NativeOptions::default();
    if let Err(e) = eframe::run_native(&args.title, native_options, Box::new(|_cc| Box::new(ViewerApp { state, gl_prog: None, gl_raw_tex: None, zoom: 1.0, pan: egui::Vec2::ZERO, dragging: false, last_drag_pos: egui::Pos2::ZERO }))) {
        return Err(eyre!("eframe initialization failed: {e}"));
    }
    Ok(())
}
