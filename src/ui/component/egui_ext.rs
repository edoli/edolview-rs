use eframe::egui::{self, Color32, Label, Response, Ui, Widget, WidgetText};

use crate::util::cv_ext::CvIntExt;

trait Join {
    fn join(&self, sep: &str) -> String;
}

impl Join for Vec<f32> {
    fn join(&self, sep: &str) -> String {
        self.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(sep)
    }
}

pub trait UiExt {
    fn data_label(&mut self, text: impl Into<WidgetText>) -> Response;
    fn label_with_colored_rect(&mut self, color: Vec<f32>, dtype: i32) -> Response;
}

impl UiExt for Ui {
    #[inline]
    fn data_label(&mut self, text: impl Into<WidgetText>) -> Response {
        Label::new(text).ui(self)
    }

    #[inline]
    fn label_with_colored_rect(&mut self, color: Vec<f32>, dtype: i32) -> Response {        
        let color32 = if color.len() == 1 {
            Color32::from_gray((color[0] * 255.0) as u8)
        } else if color.len() == 2 {
            Color32::from_rgb((color[0] * 255.0) as u8, (color[1] * 255.0) as u8, 0)
        } else if color.len() == 3 {
            Color32::from_rgb((color[0] * 255.0) as u8, (color[1] * 255.0) as u8, (color[2] * 255.0) as u8)
        } else if color.len() == 4 {
            Color32::from_rgba_premultiplied(
                (color[0] * 255.0) as u8,
                (color[1] * 255.0) as u8,
                (color[2] * 255.0) as u8,
                (color[3] * 255.0) as u8,
            )
        } else {
            Color32::BLACK
        };

        self.horizontal(|ui| {
            let rect_size = ui.available_height();
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(rect_size, rect_size), egui::Sense::click());
            ui.painter().rect_filled(rect, 4.0, color32);

            let color_text = if dtype.cv_type_is_floating() {
                format!("rgba({})", color.join(","))
            } else {
                format!(
                    "rgba({})",
                    color
                        .iter()
                        .map(|c| (((*c as f64) * dtype.alpha()) as i32).to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };

            // Right click context menu
            resp.context_menu(|ui| {
                ui.label("Color");
                ui.separator();
                // let (r, g, b, a) = (color.r(), color.g(), color.b(), color.a());
                // if ui.button("Copy #RRGGBB").clicked() {
                //     ui.ctx().copy_text(format!("#{:02X}{:02X}{:02X}", r, g, b));
                //     ui.close();
                // }
                // if ui.button("Copy rgba() ").clicked() {
                //     ui.ctx().copy_text(format!("rgba({}, {}, {}, {})", r, g, b, a));
                //     ui.close();
                // }
            });

            ui.label(&color_text);
        }).response
        
    }
}
