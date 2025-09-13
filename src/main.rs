#![allow(dead_code)]

use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use eframe::egui;
use opencv::{core, imgcodecs, imgproc, prelude::*};
use std::path::PathBuf;

mod model;
mod ui;
mod util;
use crate::{ui::ImageViewer, util::bool_ext::BoolExt};

/// Simple image viewer using OpenCV for decoding + egui for GUI.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about=None)]
struct Args {
    /// Path to the image file to open (optional)
    image: Option<PathBuf>,

    /// Window title
    #[arg(short, long, default_value_t=String::from("edolview"))]
    title: String,

    /// Max initial window size (WIDTHxHEIGHT). 0x0 keeps original size.
    #[arg(short = 's', long = "max-size", default_value = "0x0")]
    max_size: String,

    /// Start in grayscale
    #[arg(short = 'g', long)]
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
    path: Option<PathBuf>,
    original: Option<core::Mat>,
    display: Option<core::Mat>,
    grayscale: bool,
    max_w: i32,
    max_h: i32,
}

impl ImageState {
    fn empty(grayscale: bool, max_w: i32, max_h: i32) -> Self {
        Self {
            path: None,
            original: None,
            display: None,
            grayscale,
            max_w,
            max_h,
        }
    }

    fn load_from_path(&mut self, path: PathBuf) -> Result<()> {
        if !path.exists() {
            return Err(eyre!("Image does not exist: {:?}", path));
        }
        let img = imgcodecs::imread(path.to_string_lossy().as_ref(), imgcodecs::IMREAD_UNCHANGED)?;
        if img.empty() {
            return Err(eyre!("Failed to load image"));
        }
        self.path = Some(path);
        self.original = Some(img.clone());
        self.display = Some(self.process(&img)?);
        Ok(())
    }

    fn process(&self, mat: &core::Mat) -> Result<core::Mat> {
        let mut mat_f32 = core::Mat::default();
        {
            let mut tmp = core::Mat::default();
            imgproc::cvt_color(
                &mat,
                &mut tmp,
                imgproc::COLOR_BGR2RGB,
                0,
                core::AlgorithmHint::ALGO_HINT_DEFAULT,
            )?;
            tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0 / 255.0, 0.0)?;
        }
        Ok(mat_f32)
    }

    fn rebuild(&mut self) -> Result<()> {
        if let Some(orig) = &self.original {
            self.display = Some(self.process(orig)?);
        }
        Ok(())
    }
}

struct ViewerApp {
    state: ImageState,
    viewer: ImageViewer,
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("Test".to_owned()));

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button(self.state.grayscale.switch("Grayscale ✔", "Grayscale")).clicked() {
                    self.state.grayscale = !self.state.grayscale;
                    if let Err(e) = self.state.rebuild() {
                        eprintln!("Error rebuilding image: {e}");
                    }
                }
                if ui.button("Reload").clicked() {
                    if let Err(e) = self.state.rebuild() {
                        eprintln!("Error reloading: {e}");
                    }
                }
                if ui.button("Reset View").clicked() {
                    self.viewer.reset_view();
                }
                if let Some(p) = &self.state.path {
                    ui.label(format!("{}", p.display()));
                } else {
                    ui.label("(no image)");
                }
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
                if let Some(d) = &self.state.display {
                    ui.label(format!("Image size: {}x{}", d.cols(), d.rows()));
                } else {
                    ui.label("Image size: -");
                }
                ui.label(format!("Zoom: {:.2}x", self.viewer.zoom()));
            });
        });

        egui::SidePanel::right("right").show(ctx, |ui| {
            ui.heading("Controls");
            ui.label("Use mouse wheel to zoom, drag to pan.");
            ui.separator();
            ui.label("Keyboard shortcuts:");
            ui.label("  R: Reset view");
            ui.label("  G: Toggle grayscale");
            ui.label("  Q/Esc: Quit");
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(0))
            .show(ctx, |ui| {
                if let Some(d) = &self.state.display {
                    self.viewer.show_image(ui, frame, d, self.state.grayscale);
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Drag & Drop an image file or use future Open command (not implemented).");
                    });
                }
            });
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let (max_w, max_h) = parse_max_size(&args.max_size)?;
    let mut state = ImageState::empty(args.grayscale, max_w, max_h);
    if let Some(p) = args.image.clone() {
        if let Err(e) = state.load_from_path(p) {
            eprintln!("Failed to load image: {e}");
        }
    }
    let native_options = eframe::NativeOptions::default();
    if let Err(e) = eframe::run_native(
        &args.title,
        native_options,
        Box::new(|_cc| {
            Ok(Box::new(ViewerApp {
                state,
                viewer: ImageViewer::new(),
            }))
        }),
    ) {
        return Err(eyre!("eframe initialization failed: {e}"));
    }
    Ok(())
}
