use eframe::egui::{Color32, Layout, Pos2, Sense, Stroke, Ui, Vec2};

pub fn draw_multi_line_plot(ui: &mut Ui, desired_size: Vec2, series: &Vec<&Vec<f32>>, mask: &[bool]) {
    if series.is_empty() || series[0].is_empty() {
        ui.allocate_ui_with_layout(
            desired_size,
            Layout::centered_and_justified(eframe::egui::Direction::LeftToRight),
            |ui| {
                ui.label("No data to display.");
            },
        );
        return;
    }

    let max_points: usize = 256;

    let orig_len = series[0].len();
    let step: usize = if orig_len <= max_points {
        1
    } else {
        // ceil(orig_len / max_points)
        (orig_len + max_points - 1) / max_points
    };

    // Downsampling using average within each step
    #[inline]
    fn downsample_avg(xs: &[f32], step: usize) -> Vec<f32> {
        if step <= 1 {
            return xs.to_vec();
        }
        let mut out = Vec::with_capacity((xs.len() + step - 1) / step);
        let mut i = 0;
        while i < xs.len() {
            let end = (i + step).min(xs.len());
            let mut sum = 0.0f32;
            let mut cnt = 0usize;
            for v in &xs[i..end] {
                sum += *v;
                cnt += 1;
            }
            out.push(sum / (cnt as f32));
            i += step;
        }
        out
    }

    // Downsample all series according to mask
    let mut ds_series: Vec<Option<Vec<f32>>> = Vec::with_capacity(series.len());
    for (i, ys) in series.iter().enumerate() {
        if mask[i] {
            ds_series.push(Some(downsample_avg(ys, step)));
        } else {
            ds_series.push(None);
        }
    }

    // Find length of downsampled series
    let ds_len_opt = ds_series.iter().find_map(|opt| opt.as_ref().map(|v| v.len()));
    let Some(ds_len) = ds_len_opt else {
        ui.allocate_ui_with_layout(
            desired_size,
            Layout::centered_and_justified(eframe::egui::Direction::LeftToRight),
            |ui| ui.label("No visible series."),
        );
        return;
    };
    if ds_len < 2 {
        ui.allocate_ui_with_layout(
            desired_size,
            Layout::centered_and_justified(eframe::egui::Direction::LeftToRight),
            |ui| ui.label("Not enough points."),
        );
        return;
    }

    let (rect, _response) = ui.allocate_exact_size(desired_size, Sense::drag());

    let painter = ui.painter_at(rect);

    let mut y_min = f32::INFINITY;
    let mut y_max = f32::NEG_INFINITY;
    for opt in ds_series.iter() {
        if let Some(ys) = opt {
            for &y in ys {
                y_min = y_min.min(y);
                y_max = y_max.max(y);
            }
        }
    }
    if !y_min.is_finite() || !y_max.is_finite() {
        y_min = 0.0;
        y_max = 1.0;
    }
    if (y_max - y_min).abs() < f32::EPSILON {
        y_max += 1.0;
    }
    let diff = y_max - y_min;
    y_min -= 0.02 * diff;
    y_max += 0.02 * diff;

    let to_screen = |i: usize, y: f32| -> Pos2 {
        let x = i as f32 / (ds_len - 1) as f32;
        let y = (y - y_min) / (y_max - y_min);

        Pos2 {
            x: rect.left() + x * rect.width(),
            y: rect.bottom() - y * rect.height(),
        }
    };

    let colors = [
        Color32::RED,
        Color32::GREEN,
        Color32::BLUE,
        Color32::YELLOW,
        Color32::LIGHT_BLUE,
        Color32::LIGHT_GREEN,
        Color32::LIGHT_RED,
        Color32::from_rgb(200, 100, 255),
    ];

    // Draw background grid
    {
        let stroke = Stroke::new(1.0, Color32::from_gray(36));

        let num_row = (rect.height() / 32.0).ceil() as usize;
        let num_col = (rect.width() / 32.0).ceil() as usize;

        // Horizontal lines
        for i in 0..=num_row {
            let y = rect.top() + (i as f32 / num_row as f32) * (rect.height() - 2.0) + 1.0;
            painter.line_segment(
                [
                    Pos2 {
                        x: rect.left() + 1.0,
                        y,
                    },
                    Pos2 {
                        x: rect.right() + 1.0,
                        y,
                    },
                ],
                stroke,
            );
        }

        // Vertical lines
        for i in 0..=num_col {
            let x = rect.left() + (i as f32 / num_col as f32) * (rect.width() - 2.0) + 1.0;
            painter.line_segment(
                [
                    Pos2 { x, y: rect.top() + 1.0 },
                    Pos2 {
                        x,
                        y: rect.bottom() + 1.0,
                    },
                ],
                stroke,
            );
        }
    }

    for (i, opt) in ds_series.into_iter().enumerate() {
        if let Some(ys) = opt {
            let color = colors[i % colors.len()];
            let stroke = Stroke::new(1.0, color);
            for x in 1..ds_len {
                let p1 = to_screen(x - 1, ys[x - 1]);
                let p2 = to_screen(x, ys[x]);
                painter.line_segment([p1, p2], stroke);
            }
        }
    }
}
