use eframe::egui::{self, SizeHint};

pub fn load_svg_icon_texture(ctx: &egui::Context, name: impl Into<String>, svg_bytes: &[u8]) -> egui::TextureHandle {
    let options = resvg::usvg::Options::default();
    let image = load_svg_bytes(svg_bytes, &options).expect("Failed to load SVG icon");

    let size = image.size;
    assert!(
        size[0] <= 24 && size[1] <= 24,
        "SVG icon size should be at most 24x24, got {}x{}",
        size[0],
        size[1]
    );
    ctx.load_texture(name, image, Default::default())
}

pub trait IconExt {
    fn to_icon<'c>(&self, ui: &egui::Ui) -> egui::Image<'c>;
}

impl IconExt for egui::TextureHandle {
    fn to_icon<'c>(&self, ui: &egui::Ui) -> egui::Image<'c> {
        let size = self.size_vec2();
        egui::Image::from(self).fit_to_exact_size(size / ui.pixels_per_point())
    }
}

/// Load an SVG and rasterize it into an egui image.
///
/// Requires the "svg" feature.
///
/// # Errors
/// On invalid image
pub fn load_svg_bytes(svg_bytes: &[u8], options: &resvg::usvg::Options<'_>) -> Result<egui::ColorImage, String> {
    load_svg_bytes_with_size(svg_bytes, Default::default(), options)
}

/// Load an SVG and rasterize it into an egui image with a scaling parameter.
///
/// Requires the "svg" feature.
///
/// # Errors
/// On invalid image
pub fn load_svg_bytes_with_size(
    svg_bytes: &[u8],
    size_hint: SizeHint,
    options: &resvg::usvg::Options<'_>,
) -> Result<egui::ColorImage, String> {
    use egui::Vec2;
    use resvg::{
        tiny_skia::Pixmap,
        usvg::{Transform, Tree},
    };

    let rtree = Tree::from_data(svg_bytes, options).map_err(|err| err.to_string())?;

    let source_size = Vec2::new(rtree.size().width(), rtree.size().height());

    let scaled_size = match size_hint {
        SizeHint::Size {
            width,
            height,
            maintain_aspect_ratio,
        } => {
            if maintain_aspect_ratio {
                // As large as possible, without exceeding the given size:
                let mut size = source_size;
                size *= width as f32 / source_size.x;
                if size.y > height as f32 {
                    size *= height as f32 / size.y;
                }
                size
            } else {
                Vec2::new(width as _, height as _)
            }
        }
        SizeHint::Height(h) => source_size * (h as f32 / source_size.y),
        SizeHint::Width(w) => source_size * (w as f32 / source_size.x),
        SizeHint::Scale(scale) => scale.into_inner() * source_size,
    };

    let scaled_size = scaled_size.round();
    let (w, h) = (scaled_size.x as u32, scaled_size.y as u32);

    let mut pixmap = Pixmap::new(w, h).ok_or_else(|| format!("Failed to create SVG Pixmap of size {w}x{h}"))?;

    resvg::render(
        &rtree,
        Transform::from_scale(w as f32 / source_size.x, h as f32 / source_size.y),
        &mut pixmap.as_mut(),
    );

    let image =
        egui::ColorImage::from_rgba_premultiplied([w as _, h as _], pixmap.data()).with_source_size(source_size);

    Ok(image)
}
