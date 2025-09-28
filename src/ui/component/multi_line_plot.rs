use eframe::egui::{Color32, Pos2, Sense, Stroke, Ui, Vec2};

pub fn draw_multi_line_plot(ui: &mut Ui, desired_size: Vec2, series: &Vec<&Vec<f32>>) {
    if series.is_empty() || series[0].is_empty() {
        ui.label("데이터가 없습니다.");
        return;
    }

    let len = series[0].len();

    let (rect, _response) = ui.allocate_exact_size(desired_size, Sense::drag());

    let painter = ui.painter_at(rect);

    let mut y_min = f32::INFINITY;
    let mut y_max = f32::NEG_INFINITY;
    for ys in series.iter() {
        for &y in *ys {
            y_min = y_min.min(y);
            y_max = y_max.max(y);
        }
    }
    if (y_max - y_min).abs() < f32::EPSILON {
        y_max += 1.0;
    }

    let to_screen = |i: usize, y: f32| -> Pos2 {
        let x = i as f32 / (len - 1) as f32;
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

    for (i, ys) in series.iter().enumerate() {
        let color = colors[i % colors.len()];
        let stroke = Stroke::new(1.0, color);

        for x in 1..len {
            let p1 = to_screen(x - 1, ys[x - 1]);
            let p2 = to_screen(x, ys[x]);
            painter.line_segment([p1, p2], stroke);
        }
    }
}
