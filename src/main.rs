#![allow(dead_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use eframe::egui::{self, ViewportBuilder};
use opencv::{core, imgcodecs, imgproc, prelude::*};
use std::path::PathBuf;

use crate::ui::ViewerApp;

mod model;
mod res;
mod ui;
mod util;

#[cfg(debug_assertions)]
mod debug;

const ICON_DATA: &[u8] = include_bytes!("../icons/icon.png");

/// Simple image viewer using OpenCV for decoding + egui for GUI.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about=None)]
struct Args {
    /// Path to the image file to open (optional)
    image: Option<PathBuf>,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    #[cfg(target_os = "linux")]
    {
        std::env::remove_var("WAYLAND_DISPLAY");
    }

    println!("{}", core::get_build_information()?);

    let icon_raw = core::Mat::new_rows_cols_with_data(1, ICON_DATA.len() as i32, &ICON_DATA)?;
    let icon_mat_bgra = imgcodecs::imdecode(&icon_raw, imgcodecs::IMREAD_UNCHANGED)?;
    let mut icon_mat_rgba = core::Mat::default();
    imgproc::cvt_color(
        &icon_mat_bgra,
        &mut icon_mat_rgba,
        imgproc::COLOR_BGRA2RGBA,
        0,
        core::AlgorithmHint::ALGO_HINT_DEFAULT,
    )?;
    let icon_bytes = icon_mat_rgba.data_bytes();

    let icon = if let Ok(icon_bytes) = icon_bytes {
        egui::IconData {
            rgba: icon_bytes.to_vec(),
            width: icon_mat_rgba.cols() as u32,
            height: icon_mat_rgba.rows() as u32,
        }
    } else {
        eprintln!("Failed to load icon image; continuing without icon");
        egui::IconData::default()
    };

    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("edolview-rs")
            .with_inner_size(egui::vec2(1280.0, 720.0))
            .with_min_inner_size(egui::vec2(720.0, 480.0))
            .with_icon(icon),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "edolview-rs",
        native_options,
        Box::new(|_cc| Ok(Box::new(ViewerApp::new().with_path(args.image)))),
    ) {
        return Err(eyre!("eframe initialization failed: {e}"));
    }
    Ok(())
}
