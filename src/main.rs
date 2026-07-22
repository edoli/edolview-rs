#![allow(dead_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use eframe::egui::{self, ViewportBuilder};
use std::{path::PathBuf, sync::Arc};

use crate::ui::ViewerApp;

mod control;
mod model;
mod res;
mod settings;
mod supported_image;
mod ui;
mod update;
mod util;

#[cfg(debug_assertions)]
mod debug;

const ICON_DATA: &[u8] = include_bytes!("../icons/icon.png");
#[cfg(target_os = "windows")]
const WINDOWS_APP_USER_MODEL_ID: &str = "kr.edoli.edolview";

/// GPU-accelerated image viewer using image-rs for decoding and egui for GUI.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about=None)]
struct Args {
    /// Image file paths to open (optional)
    images: Vec<PathBuf>,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    set_windows_app_user_model_id();
    let args = Args::parse();

    let settings = crate::settings::AppSettings::load().unwrap_or_else(|err| {
        eprintln!("Failed to load settings: {err}");
        crate::settings::AppSettings::default()
    });

    if !args.images.is_empty() && settings.external_open_mode == crate::settings::ExternalOpenMode::ExistingWindow {
        match crate::control::try_forward_paths_to_last_active(&args.images) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(err) => eprintln!("Failed to forward files to an existing Edolview window: {err}"),
        }
    }

    #[cfg(target_os = "linux")]
    {
        std::env::remove_var("WAYLAND_DISPLAY");
    }

    let icon = match image::load_from_memory(ICON_DATA) {
        Ok(icon) => {
            let icon = icon.into_rgba8();
            egui::IconData {
                width: icon.width(),
                height: icon.height(),
                rgba: icon.into_raw(),
            }
        }
        Err(error) => {
            eprintln!("Failed to load icon image; continuing without icon: {error}");
            egui::IconData::default()
        }
    };

    let mut native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("edolview-rs")
            .with_inner_size(egui::vec2(1280.0, 720.0))
            .with_min_inner_size(egui::vec2(720.0, 480.0))
            .with_icon(icon),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    if let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut native_options.wgpu_options.wgpu_setup {
        let default_device_descriptor = Arc::clone(&setup.device_descriptor);
        setup.device_descriptor = Arc::new(move |adapter| {
            let mut descriptor = default_device_descriptor(adapter);
            let adapter_limits = adapter.limits();
            descriptor.required_limits.max_texture_dimension_2d = adapter_limits.max_texture_dimension_2d;
            descriptor.required_limits.max_storage_buffer_binding_size = adapter_limits.max_storage_buffer_binding_size;
            descriptor.required_limits.max_buffer_size = adapter_limits.max_buffer_size;
            if adapter.get_info().backend == wgpu::Backend::Dx12 {
                // Keep the initial DX12 heaps small enough for low VRAM usage while
                // allowing the allocator to grow for workloads with many resources.
                // wgpu-hal derives the host range as half of this device range.
                const MIB: u64 = 1024 * 1024;
                descriptor.memory_hints = wgpu::MemoryHints::Manual {
                    suballocated_device_memory_block_size: 40 * MIB..256 * MIB,
                };
            }
            descriptor
        });
    }

    if let Err(e) = eframe::run_native(
        "edolview-rs",
        native_options,
        Box::new(|_cc| Ok(Box::new(ViewerApp::new().with_paths(args.images)))),
    ) {
        return Err(eyre!("eframe initialization failed: {e}"));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn set_windows_app_user_model_id() {
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    let mut app_id = WINDOWS_APP_USER_MODEL_ID.encode_utf16().collect::<Vec<_>>();
    app_id.push(0);

    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
    }
}

#[cfg(not(target_os = "windows"))]
fn set_windows_app_user_model_id() {}
