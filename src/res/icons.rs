use std::sync::OnceLock;

use eframe::egui;

use crate::ui::icon::{self, IconExt};

pub const SHOW_BACKGROUND: &[u8] = include_bytes!("icons/show_background.svg");
pub const SHOW_PIXEL_VALUE: &[u8] = include_bytes!("icons/show_pixel_value.svg");
pub const SHOW_CROSSHAIR: &[u8] = include_bytes!("icons/show_crosshair.svg");

pub const SCALE_LINEAR: &[u8] = include_bytes!("icons/scale_linear.svg");
pub const SCALE_INVERSE: &[u8] = include_bytes!("icons/scale_inverse.svg");
pub const SCALE_LOG: &[u8] = include_bytes!("icons/scale_log.svg");

pub const NORMALIZE: &[u8] = include_bytes!("icons/normalize.svg");

pub struct Icons {
    show_background: OnceLock<egui::TextureHandle>,
    show_pixel_value: OnceLock<egui::TextureHandle>,
    show_crosshair: OnceLock<egui::TextureHandle>,

    scale_linear: OnceLock<egui::TextureHandle>,
    scale_inverse: OnceLock<egui::TextureHandle>,
    scale_log: OnceLock<egui::TextureHandle>,

    normalize: OnceLock<egui::TextureHandle>,
}

impl Icons {
    pub fn new() -> Self {
        Self {
            show_background: OnceLock::new(),
            show_pixel_value: OnceLock::new(),
            show_crosshair: OnceLock::new(),

            scale_linear: OnceLock::new(),
            scale_inverse: OnceLock::new(),
            scale_log: OnceLock::new(),

            normalize: OnceLock::new(),
        }
    }

    #[inline]
    pub fn get_show_background<'c>(&self, ctx: &egui::Context) -> egui::Image<'c> {
        self.show_background
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "show_background", SHOW_BACKGROUND))
            .to_icon(ctx)
    }

    #[inline]
    pub fn get_show_pixel_value<'c>(&self, ctx: &egui::Context) -> egui::Image<'c> {
        self.show_pixel_value
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "show_pixel_value", SHOW_PIXEL_VALUE))
            .to_icon(ctx)
    }
    #[inline]
    pub fn get_show_crosshair<'c>(&self, ctx: &egui::Context) -> egui::Image<'c> {
        self.show_crosshair
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "show_crosshair", SHOW_CROSSHAIR))
            .to_icon(ctx)
    }

    #[inline]
    pub fn get_scale_linear<'c>(&self, ctx: &egui::Context) -> egui::Image<'c> {
        self.scale_linear
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "scale_linear", SCALE_LINEAR))
            .to_icon(ctx)
    }

    #[inline]
    pub fn get_scale_inverse<'c>(&self, ctx: &egui::Context) -> egui::Image<'c> {
        self.scale_inverse
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "scale_inverse", SCALE_INVERSE))
            .to_icon(ctx)
    }

    #[inline]
    pub fn get_scale_log<'c>(&self, ctx: &egui::Context) -> egui::Image<'c> {
        self.scale_log
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "scale_log", SCALE_LOG))
            .to_icon(ctx)
    }

    #[inline]
    pub fn get_normalize<'c>(&self, ctx: &egui::Context) -> egui::Image<'c> {
        self.normalize
            .get_or_init(|| icon::load_svg_icon_texture(ctx, "normalize", NORMALIZE))
            .to_icon(ctx)
    }
}
