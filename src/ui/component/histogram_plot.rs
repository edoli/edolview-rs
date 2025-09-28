use eframe::egui::{self, pos2, Color32, Rect, Sense, Vec2};

pub fn draw_histogram(ui: &mut egui::Ui, series: &Vec<&[f32]>, mask: &[bool], max_freq: f32, size_px: Vec2) {
    let (rect, _) = ui.allocate_exact_size(size_px, Sense::hover());
    let painter = ui.painter_at(rect);

    if max_freq <= 0.0 {
        return;
    }

    let bins = series.get(0).map_or(0, |s| s.len());
    if bins == 0 {
        return;
    }

    let full_w = rect.width();
    let baseline_y = rect.bottom();
    let left_x = rect.left();

    let bin_width = full_w / bins as f32;
    let inner_w = bin_width.max(0.0);

    let colors = [
        Color32::from_rgba_unmultiplied(255, 0, 0, 150),
        Color32::from_rgba_unmultiplied(0, 255, 0, 150),
        Color32::from_rgba_unmultiplied(0, 0, 255, 150),
    ];

    let draw_series = |series: &[f32], color: Color32| {
        for i in 0..bins {
            let v = *series.get(i).unwrap_or(&0.0);
            if v <= 0.0 {
                continue;
            }
            let h_px = (v / max_freq).clamp(0.0, 1.0) * rect.height();
            if h_px <= 0.0 {
                continue;
            }

            let x0 = left_x + i as f32 * bin_width;
            let x1 = x0 + inner_w;
            let y_top = baseline_y - h_px;

            let x0c = x0.max(rect.left());
            let x1c = x1.min(rect.right());
            if x1c <= x0c {
                continue;
            }
            let y0c = y_top.max(rect.top());
            let y1c = baseline_y.min(rect.bottom());
            if y1c <= y0c {
                continue;
            }

            let r = Rect::from_min_max(pos2(x0c, y0c), pos2(x1c, y1c));
            painter.rect_filled(r, 0.0, color);
        }
    };

    series.iter().take(3).enumerate().for_each(|(i, s)| {
        if mask[i] {
            draw_series(s, colors[i])
        }
    });
}
