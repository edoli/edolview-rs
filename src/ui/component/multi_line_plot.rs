use std::sync::Arc;

use eframe::egui::epaint::Shape;
use eframe::egui::Galley;
use eframe::egui::{Color32, CornerRadius, Layout, Pos2, Rect, Sense, Stroke, TextStyle, Ui, Vec2};

use super::{CsvExportAction, CsvExportPayload};
use crate::util::series::{build_indexed_csv, channel_label, SeriesRef};

// Downsampling using average within each step
#[inline]
fn downsample_avg(xs: &[f64], step: usize) -> Vec<f64> {
    if step <= 1 {
        return xs.to_vec();
    }
    let mut out = Vec::with_capacity((xs.len() + step - 1) / step);
    let mut i = 0;
    while i < xs.len() {
        let end = (i + step).min(xs.len());
        let mut sum = 0.0;
        let mut cnt = 0usize;
        for v in &xs[i..end] {
            sum += *v;
            cnt += 1;
        }
        out.push(sum / (cnt as f64));
        i += step;
    }
    out
}

pub fn draw_multi_line_plot(
    ui: &mut Ui,
    desired_size: Vec2,
    series: SeriesRef<'_, f64>,
    mask: &[bool],
    alpha_scale: f64,
    position_label: &'static str,
    position_offset: i32,
) -> Option<CsvExportAction> {
    if series.is_empty() || series.first_len() == 0 {
        ui.allocate_ui_with_layout(
            desired_size,
            Layout::centered_and_justified(eframe::egui::Direction::LeftToRight),
            |ui| {
                ui.label("No data to display.");
            },
        );
        return None;
    }

    let max_points: usize = 256;

    let orig_len = series.first_len();
    let step: usize = if orig_len <= max_points {
        1
    } else {
        // ceil(orig_len / max_points)
        (orig_len + max_points - 1) / max_points
    };

    // Downsample all series according to mask
    let mut ds_series: Vec<Option<Vec<f64>>> = Vec::with_capacity(series.len());
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
        return None;
    };
    if ds_len < 2 {
        ui.allocate_ui_with_layout(
            desired_size,
            Layout::centered_and_justified(eframe::egui::Direction::LeftToRight),
            |ui| ui.label("Not enough points."),
        );
        return None;
    }

    // Allocate the plotting area and keep the response for hover interactivity
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click_and_drag());
    let mut export_action = None;

    let painter = ui.painter_at(rect);

    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
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
    if (y_max - y_min).abs() < f64::EPSILON {
        y_max += 1.0;
    }
    let diff = y_max - y_min;
    y_min -= 0.02 * diff;
    y_max += 0.02 * diff;

    let to_screen = |i: usize, y: f64| -> Pos2 {
        let x = i as f32 / (ds_len - 1) as f32;
        let y = (y - y_min) / (y_max - y_min);

        Pos2 {
            x: rect.left() + x * rect.width(),
            y: rect.bottom() - y as f32 * rect.height(),
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

    for (i, opt) in ds_series.iter().enumerate() {
        if let Some(ys) = opt.as_ref() {
            let color = colors[i % colors.len()];
            let stroke = Stroke::new(1.0, color);
            for x in 1..ds_len {
                let p1 = to_screen(x - 1, ys[x - 1]);
                let p2 = to_screen(x, ys[x]);
                painter.line_segment([p1, p2], stroke);
            }
        }
    }

    response.context_menu(|ui| {
        if ui.button("Copy Data (CSV)").clicked() {
            export_action = Some(CsvExportAction::Copy(CsvExportPayload::new(
                "plot data",
                "multi-line-plot.csv",
                build_indexed_csv(
                    position_label,
                    Some(&format!("{position_label}_absolute")),
                    Some(position_offset),
                    &series.scaled(alpha_scale),
                    mask,
                ),
            )));
            ui.close();
        }
        if ui.button("Save Data as CSV...").clicked() {
            export_action = Some(CsvExportAction::Save(CsvExportPayload::new(
                "plot data",
                "multi-line-plot.csv",
                build_indexed_csv(
                    position_label,
                    Some(&format!("{position_label}_absolute")),
                    Some(position_offset),
                    &series.scaled(alpha_scale),
                    mask,
                ),
            )));
            ui.close();
        }
    });

    // Hover interactivity: vertical dashed cursor line and tooltip with x/value(s)
    if let Some(mouse_pos) = response.hover_pos() {
        if rect.contains(mouse_pos) {
            let x_norm = ((mouse_pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            // Snap to nearest downsampled index
            let ds_idx =
                ((x_norm * (ds_len.saturating_sub(1) as f32)).round() as usize).clamp(0, ds_len.saturating_sub(1));
            // Map snapped downsample index back to approximate original center index for display
            let approx_orig_idx = (ds_idx.saturating_mul(step) + step / 2).min(orig_len.saturating_sub(1));

            // Draw a dashed vertical line at the cursor x
            let x = rect.left() + (ds_idx as f32 / (ds_len.saturating_sub(1) as f32)) * rect.width();
            let dash_len = 6.0f32;
            let gap_len = 4.0f32;
            let mut y0 = rect.top();
            let stroke = Stroke::new(1.0, Color32::from_gray(180));
            while y0 < rect.bottom() {
                let y1 = (y0 + dash_len).min(rect.bottom());
                painter.line_segment([Pos2 { x, y: y0 }, Pos2 { x, y: y1 }], stroke);
                y0 += dash_len + gap_len;
            }

            // Prepare tooltip lines: first x, then each visible series value
            let mut lines: Vec<(String, Color32)> = Vec::new();
            let absolute_idx = position_offset + approx_orig_idx as i32;
            lines.push((
                format!("{position_label}: {} ({absolute_idx})", approx_orig_idx),
                Color32::WHITE,
            ));
            let total_channels = series.len();
            for (i, opt) in ds_series.iter().enumerate() {
                if !mask.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let color = colors[i % colors.len()];
                if let Some(ys) = opt.as_ref() {
                    let val = ys.get(ds_idx).copied().unwrap_or(f64::NAN) * alpha_scale;
                    lines.push((format!("{}: {:.4}", channel_label(i, total_channels), val), color));
                }
            }

            // Determine tooltip size and precompute galleys (avoid painting inside fonts lock)
            let font_id = TextStyle::Monospace.resolve(ui.style());
            let padding = Vec2::new(8.0, 6.0);
            let mut galleys: Vec<(Arc<Galley>, Color32)> = Vec::with_capacity(lines.len());
            let (max_w, total_h) = ui.fonts(|fonts| {
                let mut max_w = 0.0f32;
                let mut total_h = 0.0f32;
                for (text, color) in &lines {
                    let galley = fonts.layout_no_wrap(text.clone(), font_id.clone(), *color);
                    let sz = galley.size();
                    max_w = max_w.max(sz.x);
                    total_h += sz.y;
                    galleys.push((galley, *color));
                }
                (max_w, total_h)
            });
            let tooltip_size = Vec2::new(max_w + padding.x * 2.0, total_h + padding.y * 2.0);

            // Place tooltip near cursor, clamped to rect
            let mut tip_pos = Pos2 {
                x: (mouse_pos.x + 12.0).min(rect.right() - tooltip_size.x),
                y: (mouse_pos.y + 12.0).min(rect.bottom() - tooltip_size.y),
            };
            // Ensure it doesn't go above/left
            if tip_pos.x < rect.left() {
                tip_pos.x = rect.left();
            }
            if tip_pos.y < rect.top() {
                tip_pos.y = rect.top();
            }

            let tip_rect = Rect::from_min_size(tip_pos, tooltip_size);

            // Draw tooltip background and border using Shapes for compatibility
            painter.add(Shape::rect_filled(
                tip_rect,
                CornerRadius::same(4),
                Color32::from_black_alpha(180),
            ));
            // Border as 4 lines (avoids version-specific rect_stroke signature)
            let border = Stroke::new(1.0, Color32::from_gray(80));
            painter.line_segment(
                [
                    Pos2 {
                        x: tip_rect.left(),
                        y: tip_rect.top(),
                    },
                    Pos2 {
                        x: tip_rect.right(),
                        y: tip_rect.top(),
                    },
                ],
                border,
            );
            painter.line_segment(
                [
                    Pos2 {
                        x: tip_rect.right(),
                        y: tip_rect.top(),
                    },
                    Pos2 {
                        x: tip_rect.right(),
                        y: tip_rect.bottom(),
                    },
                ],
                border,
            );
            painter.line_segment(
                [
                    Pos2 {
                        x: tip_rect.right(),
                        y: tip_rect.bottom(),
                    },
                    Pos2 {
                        x: tip_rect.left(),
                        y: tip_rect.bottom(),
                    },
                ],
                border,
            );
            painter.line_segment(
                [
                    Pos2 {
                        x: tip_rect.left(),
                        y: tip_rect.bottom(),
                    },
                    Pos2 {
                        x: tip_rect.left(),
                        y: tip_rect.top(),
                    },
                ],
                border,
            );

            // Draw each line of text with its color
            // Draw each line of text with its color (after fonts lock released)
            let mut cursor = Pos2 {
                x: tip_rect.left() + padding.x,
                y: tip_rect.top() + padding.y,
            };
            for (galley, color) in galleys.into_iter() {
                let line_h = galley.size().y;
                painter.galley(cursor, galley, color);
                cursor.y += line_h;
            }
        }
    }

    export_action
}
