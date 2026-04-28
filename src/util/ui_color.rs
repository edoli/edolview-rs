use eframe::egui::{Color32, Visuals};

fn srgb_channel_to_linear(channel: u8) -> f32 {
    let channel = channel as f32 / 255.0;
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn relative_luminance(color: Color32) -> f32 {
    let r = srgb_channel_to_linear(color.r());
    let g = srgb_channel_to_linear(color.g());
    let b = srgb_channel_to_linear(color.b());
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn contrast_ratio(foreground: Color32, background: Color32) -> f32 {
    let fg = relative_luminance(foreground);
    let bg = relative_luminance(background);
    let (lighter, darker) = if fg >= bg { (fg, bg) } else { (bg, fg) };
    (lighter + 0.05) / (darker + 0.05)
}

fn pick_label_color(backgrounds: &[Color32], candidates: &[Color32]) -> Color32 {
    candidates
        .iter()
        .copied()
        .max_by(|a, b| {
            let a_score = backgrounds
                .iter()
                .copied()
                .map(|background| contrast_ratio(*a, background))
                .fold(f32::INFINITY, f32::min);
            let b_score = backgrounds
                .iter()
                .copied()
                .map(|background| contrast_ratio(*b, background))
                .fold(f32::INFINITY, f32::min);
            a_score.partial_cmp(&b_score).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(Color32::WHITE)
}

pub fn statistics_label_colors(visuals: &Visuals) -> (Color32, Color32) {
    let backgrounds = [visuals.extreme_bg_color, visuals.faint_bg_color];
    let min_candidates = [
        Color32::from_rgb(0, 102, 204),
        Color32::from_rgb(0, 122, 255),
        Color32::from_rgb(96, 178, 255),
        Color32::from_rgb(140, 210, 255),
    ];
    let max_candidates = [
        Color32::from_rgb(176, 32, 32),
        Color32::from_rgb(214, 48, 49),
        Color32::from_rgb(255, 110, 110),
        Color32::from_rgb(255, 150, 120),
    ];

    (
        pick_label_color(&backgrounds, &min_candidates),
        pick_label_color(&backgrounds, &max_candidates),
    )
}
