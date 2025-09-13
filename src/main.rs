#![allow(dead_code)]

use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use eframe::egui;
use opencv::{core, imgcodecs, imgproc, prelude::*};
use rfd::FileDialog;
use std::fs;
use std::path::PathBuf;

use crate::{
    model::{Image, MatImage},
    ui::ImageViewer,
    util::bool_ext::BoolExt,
};

mod model;
mod ui;
mod util;

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
    display: Option<MatImage>,
    grayscale: bool,
    max_w: i32,
    max_h: i32,
}

impl ImageState {
    fn empty(grayscale: bool, max_w: i32, max_h: i32) -> Self {
        Self {
            path: None,
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

        // Read image using imread fails on paths with non-ASCII characters.
        // let img = imgcodecs::imread(path.to_string_lossy().as_ref(), imgcodecs::IMREAD_UNCHANGED)?;

        let data = fs::read(&path).map_err(|e| eyre!("Failed to read file bytes: {e}"))?;
        let data_mat = core::Mat::new_rows_cols_with_data(1, data.len() as i32, &data)?;
        let img = imgcodecs::imdecode(&data_mat, imgcodecs::IMREAD_UNCHANGED)?;

        if img.empty() {
            return Err(eyre!("Failed to load image"));
        }
        self.path = Some(path);
        self.display = Some(self.process(&img)?);
        Ok(())
    }

    fn process(&self, mat: &core::Mat) -> Result<MatImage> {
        let mut mat_f32 = core::Mat::default();
        {
            let mut tmp = core::Mat::default();
            
            let channels = mat.channels();
            let color_convert = match mat.channels() {
                1 => imgproc::COLOR_BGR2RGB,
                3 => imgproc::COLOR_BGR2RGB,
                4 => imgproc::COLOR_BGRA2RGBA,
                _ => {
                    return Err(eyre!("Unsupported image channels: {}", channels));
                }
            };

            imgproc::cvt_color(
                &mat,
                &mut tmp,
                color_convert,
                0,
                core::AlgorithmHint::ALGO_HINT_DEFAULT,
            )?;
            
            let dtype = tmp.depth();
            match dtype {
                core::CV_8U => {
                    tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0 / 255.0, 0.0)?;
                }
                core::CV_8S => {
                    tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0 / 127.0, 0.0)?;
                }
                core::CV_16U => {
                    tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0 / 65535.0, 0.0)?;
                }
                core::CV_16S => {
                    tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0 / 32767.0, 0.0)?;
                }
                core::CV_32S => {
                    tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0 / 2147483647.0, 0.0)?;
                }
                core::CV_32F => {
                    mat_f32 = tmp;
                }
                core::CV_64F => {
                    tmp.convert_to(&mut mat_f32, core::CV_32F, 1.0, 0.0)?;
                }
                _ => {
                    return Err(eyre!("Unsupported image depth: {}", dtype));
                }
            }
        }
        Ok(MatImage::new(mat_f32))
    }
}

struct ViewerApp {
    state: ImageState,
    viewer: ImageViewer,
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("Test".to_owned()));

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
                            self.viewer.reset_view();
                            break;
                        }
                        Err(e) => {
                            eprintln!("Failed to load dropped file: {e}");
                        }
                    }
                }
            }
        }

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
                if ui.button(self.state.grayscale.switch("Grayscale ✔", "Grayscale")).clicked() {
                    self.state.grayscale = !self.state.grayscale;
                }
                if ui.button("Reset View").clicked() {
                    self.viewer.reset_view();
                }
                if let Some(p) = &self.state.path {
                    ui.label(format!("{}", p.display()));
                } else {
                    ui.label("(no image)");
                }
            });
        });

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(d) = &self.state.display {
                    ui.label(format!("Image size: {}x{}", d.spec().width, d.spec().height));
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
                        ui.label("Drag & Drop an image file here.");
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
