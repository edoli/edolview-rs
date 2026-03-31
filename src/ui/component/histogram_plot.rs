use std::sync::Arc;

use eframe::egui::epaint::Shape;
use eframe::egui::Galley;
use eframe::egui::{self, pos2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, TextStyle, Vec2};

use super::{CsvExportAction, CsvExportPayload};
use crate::util::series::{build_indexed_csv, SeriesRef};

pub fn draw_histogram(
    ui: &mut egui::Ui,
    desired_size: Vec2,
    series: SeriesRef<'_, f32>,
    mask: &[bool],
    max_freq: f32,
) -> Option<CsvExportAction> {
    if series.is_empty() || series.first_len() == 0 || mask.iter().all(|&m| !m) {
        ui.allocate_ui_with_layout(
            desired_size,
            egui::Layout::centered_and_justified(eframe::egui::Direction::LeftToRight),
            |ui| {
                ui.label("No data to display.");
            },
        );
        return None;
    }

    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());
    let painter = ui.painter_at(rect);
    let mut export_action = None;

    if max_freq <= 0.0 {
        return None;
    }

    let bins = series.first_len();
    if bins == 0 {
        return None;
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
        Color32::from_rgba_unmultiplied(255, 255, 0, 150),
    ];

    let format_hist_value = |value: f32| {
        if (value - value.round()).abs() < 0.0001 {
            format!("{value:.0}")
        } else {
            format!("{value:.4}")
        }
    };

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

    series.iter().take(4).enumerate().for_each(|(i, s)| {
        if mask[i] {
            draw_series(s, colors[i])
        }
    });

    response.context_menu(|ui| {
        if ui.button("Copy Data (CSV)").clicked() {
            export_action = Some(CsvExportAction::Copy(CsvExportPayload::new(
                "histogram data",
                "histogram.csv",
                build_indexed_csv("bin", None, None, "s", &series, mask),
            )));
            ui.close();
        }
        if ui.button("Save Data as CSV...").clicked() {
            export_action = Some(CsvExportAction::Save(CsvExportPayload::new(
                "histogram data",
                "histogram.csv",
                build_indexed_csv("bin", None, None, "s", &series, mask),
            )));
            ui.close();
        }
    });

    if let Some(mouse_pos) = response.hover_pos() {
        if rect.contains(mouse_pos) {
            let x_norm = ((mouse_pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let bin_idx = ((x_norm * bins as f32).floor() as usize).min(bins.saturating_sub(1));
            let x = rect.left() + (bin_idx as f32 + 0.5) * bin_width;

            let dash_len = 6.0f32;
            let gap_len = 4.0f32;
            let mut y0 = rect.top();
            let stroke = Stroke::new(1.0, Color32::from_gray(180));
            while y0 < rect.bottom() {
                let y1 = (y0 + dash_len).min(rect.bottom());
                painter.line_segment([Pos2 { x, y: y0 }, Pos2 { x, y: y1 }], stroke);
                y0 += dash_len + gap_len;
            }

            let mut lines: Vec<(String, Color32)> = Vec::new();
            lines.push((format!("bin: {bin_idx}"), Color32::WHITE));
            for (i, channel) in series.iter().take(4).enumerate() {
                if !mask.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let value = channel.get(bin_idx).copied().unwrap_or(0.0);
                lines.push((format!("s{i}: {}", format_hist_value(value)), colors[i % colors.len()]));
            }

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

            let mut tip_pos = Pos2 {
                x: (mouse_pos.x + 12.0).min(rect.right() - tooltip_size.x),
                y: (mouse_pos.y + 12.0).min(rect.bottom() - tooltip_size.y),
            };
            if tip_pos.x < rect.left() {
                tip_pos.x = rect.left();
            }
            if tip_pos.y < rect.top() {
                tip_pos.y = rect.top();
            }

            let tip_rect = Rect::from_min_size(tip_pos, tooltip_size);
            painter.add(Shape::rect_filled(
                tip_rect,
                CornerRadius::same(4),
                Color32::from_black_alpha(180),
            ));

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

            let mut cursor = Pos2 {
                x: tip_rect.left() + padding.x,
                y: tip_rect.top() + padding.y,
            };
            for (galley, color) in galleys {
                let line_h = galley.size().y;
                painter.galley(cursor, galley, color);
                cursor.y += line_h;
            }
        }
    }

    export_action
}
