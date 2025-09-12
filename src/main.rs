use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use eframe::{egui};
use opencv::{core, imgcodecs, imgproc, prelude::*};
use std::path::PathBuf;

mod model;
mod ui;
mod util;
use crate::{
    ui::ImageViewer,
};

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
    grayscale: bool,
    max_w: i32,
    max_h: i32,
}

impl ImageState {
    fn load(path: PathBuf, grayscale: bool, max_w: i32, max_h: i32) -> Result<Self> {
        if !path.exists() { return Err(eyre!("Image does not exist: {:?}", path)); }
        let img = imgcodecs::imread(path.to_string_lossy().as_ref(), imgcodecs::IMREAD_UNCHANGED)?;
        if img.empty() { return Err(eyre!("Failed to load image")); }
        let mut state = ImageState { path, original: img.clone(), display: core::Mat::default(), grayscale, max_w, max_h };
        state.display = state.process(&img)?;
        Ok(state)
    }

    fn process(&self, img: &core::Mat) -> Result<core::Mat> {
        let mut out = img.clone();
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
        Ok(())
    }
}

struct ViewerApp {
    state: ImageState,
    viewer: ImageViewer,
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button(if self.state.grayscale { "Grayscale ✔" } else { "Grayscale" }).clicked() {
                    self.state.grayscale = !self.state.grayscale;
                    if let Err(e) = self.state.rebuild() { eprintln!("Error rebuilding image: {e}"); }
                }
                if ui.button("Reload").clicked() {
                    if let Err(e) = self.state.rebuild() { eprintln!("Error reloading: {e}"); }
                }
                if ui.button("Reset View").clicked() {
                    self.viewer.reset_view();
                }
                ui.label(format!("{}", self.state.path.display()));
                let response = ui.add(egui::widgets::Label::new("Right-click me!").sense(egui::Sense::click()));
                response.context_menu(|ui| {
                    if ui.button("Close the menu").clicked() {
                        ui.close();
                    }
                });
            });
        });

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Image size: {}x{}", self.state.display.cols(), self.state.display.rows()));
                ui.label(format!("Zoom: {:.2}x", self.viewer.zoom()));
            });
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(0))
            .show(ctx, |ui| {
                self.viewer.show_image(
                    ui,
                    frame,
                    &self.state.display,
                    self.state.grayscale,
                );
            });
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let (max_w, max_h) = parse_max_size(&args.max_size)?;
    let state = ImageState::load(args.image.clone(), args.grayscale, max_w, max_h)?;
    let native_options = eframe::NativeOptions::default();
    if let Err(e) = eframe::run_native(&args.title, native_options, Box::new(|_cc| Ok(Box::new(ViewerApp { state, viewer: ImageViewer::new() })))) {
        return Err(eyre!("eframe initialization failed: {e}"));
    }
    Ok(())
}
