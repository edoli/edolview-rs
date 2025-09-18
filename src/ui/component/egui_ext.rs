use eframe::egui::{self, Color32, ComboBox, InnerResponse, Label, Response, Ui, Widget, WidgetText};

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
        })
        .response
    }
}

pub trait RespExt {
    fn hover_scroll<T: PartialEq + Clone>(self, ui: &Ui, values: &Vec<T>, current: &mut T, is_cycle: bool) -> Self;
}

impl<R> RespExt for InnerResponse<R> {
    fn hover_scroll<T: PartialEq + Clone>(self, ui: &Ui, values: &Vec<T>, current: &mut T, is_cycle: bool) -> Self {
        if self.response.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);

            if scroll.abs() > 0.0 {
                if let Some(cur_idx) = values.iter().position(|n| n == current) {
                    let mut new_idx = cur_idx as isize + if scroll < 0.0 { 1 } else { -1 };
                    if new_idx < 0 {
                        new_idx = if is_cycle { (values.len() - 1) as isize } else { 0 };
                    } else if new_idx as usize >= values.len() {
                        new_idx = if is_cycle { 0 } else { (values.len() - 1) as isize };
                    }
                    if new_idx as usize != cur_idx {
                        if let Some(new_value) = values.get(new_idx as usize) {
                            *current = new_value.clone();
                            // 필요 시 뷰 갱신
                            ui.ctx().request_repaint();
                        }
                    }
                }
            }
        }
        self
    }
}

pub trait ComboBoxExt {
    fn combo(self, ui: &mut Ui, selected_text: &mut String, list: &Vec<String>) -> InnerResponse<Option<()>>;
    fn combo_i32(self, ui: &mut Ui, selected_text: &mut i32, list: &Vec<i32>) -> InnerResponse<Option<()>>;
}

impl ComboBoxExt for ComboBox {
    fn combo(self, ui: &mut Ui, selected_text: &mut String, list: &Vec<String>) -> InnerResponse<Option<()>> {
        self.selected_text(selected_text.as_str())
            .show_ui(ui, |ui| {
                for name in list {
                    ui.selectable_value(selected_text, name.clone(), name);
                }
            })
            .hover_scroll(ui, list, selected_text, false)
    }
    
    fn combo_i32(self, ui: &mut Ui, selected_text: &mut i32, list: &Vec<i32>) -> InnerResponse<Option<()>> {
        self.selected_text(selected_text.to_string())
            .show_ui(ui, |ui| {
                for index in list {
                    ui.selectable_value(selected_text, *index, index.to_string());
                }
            })
            .hover_scroll(ui, list, selected_text, false)
    }
}
