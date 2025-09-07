use clap::Parser;
use color_eyre::eyre::{Result, eyre};
use opencv::{core, highgui, imgcodecs, imgproc, prelude::*};
use std::path::PathBuf;

/// Simple OpenCV based image viewer (supports basic formats)
#[derive(Parser, Debug)]
#[command(author, version, about, long_about=None)]
struct Args {
    /// Path to the image file to open
    image: PathBuf,

    /// Optional window name
    #[arg(short, long, default_value_t=String::from("edolview"))]
    title: String,

    /// Fit-to-screen maximum window size (WIDTHxHEIGHT), ex: 1280x720. 0 means keep original.
    #[arg(short = 's', long = "max-size", default_value = "0x0")]
    max_size: String,

    /// Convert to grayscale
    #[arg(short='g', long)]
    grayscale: bool,
}

fn parse_max_size(spec: &str) -> Result<(i32,i32)> {
    if spec == "0x0" { return Ok((0,0)); }
    let parts: Vec<_> = spec.split('x').collect();
    if parts.len() != 2 { return Err(eyre!("Invalid size format. Use WIDTHxHEIGHT")); }
    let w: i32 = parts[0].parse()?;
    let h: i32 = parts[1].parse()?;
    Ok((w,h))
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let (max_w, max_h) = parse_max_size(&args.max_size)?;

    if !args.image.exists() { return Err(eyre!("Image does not exist: {:?}", args.image)); }

    let img = imgcodecs::imread(args.image.to_string_lossy().as_ref(), imgcodecs::IMREAD_UNCHANGED)?;
    if img.empty() { return Err(eyre!("Failed to load image")); }
    let mut display = img;

    if args.grayscale {
        let mut gray = core::Mat::default();
        imgproc::cvt_color(&display, &mut gray, imgproc::COLOR_BGR2GRAY, 0, core::AlgorithmHint::ALGO_HINT_DEFAULT)?;
        display = gray;
    }

    if max_w > 0 && max_h > 0 {
        let size = display.size()?;
        let (w,h) = (size.width, size.height);
        if w > max_w || h > max_h {
            let scale = f64::min(max_w as f64 / w as f64, max_h as f64 / h as f64);
            let new_w = (w as f64 * scale).round() as i32;
            let new_h = (h as f64 * scale).round() as i32;
            let mut resized = core::Mat::default();
            imgproc::resize(&display, &mut resized, core::Size::new(new_w, new_h), 0.0, 0.0, imgproc::INTER_LINEAR)?;
            display = resized;
        }
    }

    highgui::named_window(&args.title, highgui::WINDOW_AUTOSIZE)?;
    highgui::imshow(&args.title, &display)?;

    println!("Press ESC or Q to quit, or N/P to do nothing (placeholder for future navigation)");
    loop {
        let key = highgui::wait_key(30)?; // milliseconds
        match key {
            27 | 113 => break, // ESC or 'q'
            -1 => {},
            _ => {},
        }
    }
    Ok(())
}
