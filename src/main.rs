#![allow(dead_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use eframe::egui::{self, ViewportBuilder};
use std::path::PathBuf;

use crate::ui::ViewerApp;

mod model;
mod res;
mod ui;
mod util;

/// Simple image viewer using OpenCV for decoding + egui for GUI.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about=None)]
struct Args {
    /// Path to the image file to open (optional)
    image: Option<PathBuf>,
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

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    #[cfg(target_os = "linux")]
    {
        std::env::remove_var("WAYLAND_DISPLAY");
    }

    println!("{}", opencv::core::get_build_information()?);
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("edolview-rs")
            .with_inner_size(egui::vec2(1280.0, 720.0)),
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
