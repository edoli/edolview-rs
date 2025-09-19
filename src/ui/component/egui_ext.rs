use eframe::egui::{self, Color32, ComboBox, InnerResponse, Label, Rangef, Response, Ui, Widget, WidgetText};

use crate::util::cv_ext::CvIntExt;

trait Join {
    fn join(&self, sep: &str) -> String;
}

impl Join for Vec<f32> {
    fn join(&self, sep: &str) -> String {
        self.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(sep)
    }
}

#[derive(Clone, Debug, Copy)]
pub enum Size {
    /// Absolute size in points, with a given range of allowed sizes to resize within.
    Absolute { initial: f32, range: Rangef },

    /// Relative size relative to all available space.
    Relative { fraction: f32, range: Rangef },

    /// Multiple remainders each get the same space.
    Remainder { weight: f32, range: Rangef },
}

impl Size {
    pub fn exact(initial: f32) -> Self {
        Self::Absolute {
            initial,
            range: Rangef::new(initial, initial),
        }
    }

    pub fn initial(initial: f32) -> Self {
        Self::Absolute {
            initial,
            range: Rangef::new(0.0, f32::INFINITY),
        }
    }

    pub fn relative(fraction: f32) -> Self {
        Self::Relative {
            fraction,
            range: Rangef::new(0.0, f32::INFINITY),
        }
    }

    pub fn remainder(weight: f32) -> Self {
        Self::Remainder {
            weight,
            range: Rangef::new(0.0, f32::INFINITY),
        }
    }

    #[inline]
    pub fn at_least(mut self, minimum: f32) -> Self {
        self.range_mut().min = minimum;
        self
    }

    /// Won't grow above this size (in points).
    #[inline]
    pub fn at_most(mut self, maximum: f32) -> Self {
        self.range_mut().max = maximum;
        self
    }

    #[inline]
    pub fn with_range(mut self, range: Rangef) -> Self {
        *self.range_mut() = range;
        self
    }

    pub fn range(self) -> Rangef {
        match self {
            Self::Absolute { range, .. } | Self::Relative { range, .. } | Self::Remainder { range, .. } => range,
        }
    }

    pub fn range_mut(&mut self) -> &mut Rangef {
        match self {
            Self::Absolute { range, .. } | Self::Relative { range, .. } | Self::Remainder { range, .. } => range,
        }
    }
}

pub trait UiExt {
    fn data_label(&mut self, text: impl Into<WidgetText>) -> Response;
    fn label_with_colored_rect(&mut self, color: Vec<f32>, dtype: i32) -> Response;
    fn text_edit_t<T: std::fmt::Display + std::str::FromStr>(&mut self, value: &mut T) -> Response;
    fn calc_sizes(&self, sizes: &[Size]) -> Vec<f32>;
    fn columns_sized<R>(&mut self, sizes: &[Size], add_contents: impl FnOnce(&mut [Self]) -> R) -> R;
    fn columns_sized_dyn<'c, R>(&mut self, sizes: &[Size], add_contents: Box<dyn FnOnce(&mut [Self]) -> R + 'c>) -> R where Self: Sized;
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

    fn text_edit_t<T: std::fmt::Display + std::str::FromStr>(&mut self, value: &mut T) -> Response {
        let mut text = value.to_string();
        let resp = self.text_edit_singleline(&mut text);
        if resp.changed() {
            if let Ok(v) = text.parse::<T>() {
                *value = v;
            }
        }
        resp
    }

    fn calc_sizes(&self, sizes: &[Size]) -> Vec<f32> {
        let total_width = self.available_width();
        let spacing = self.spacing().item_spacing.x;

        let mut results = vec![0.0; sizes.len()];

        let mut total_absolute = 0.0;
        let mut total_relative_fraction = 0.0;
        let mut total_remainders = 0.0;

        for (i, size) in sizes.iter().enumerate() {
            match size {
                Size::Absolute { initial, range } => {
                    let clamped = initial.clamp(range.min, range.max);
                    results[i] = clamped;
                    total_absolute += clamped;
                }
                Size::Relative { fraction, range: _ } => {
                    total_relative_fraction += *fraction;
                }
                Size::Remainder { weight, range: _ } => {
                    total_remainders += *weight;
                }
            }
        }

        let remaining_space = (total_width - total_absolute).max(0.0);

        if total_relative_fraction > 0.0 {
            for (i, size) in sizes.iter().enumerate() {
                if let Size::Relative { fraction, range } = size {
                    let allocated = (fraction / total_relative_fraction) * remaining_space;
                    let clamped = allocated.clamp(range.min, range.max);
                    results[i] = clamped;
                }
            }
        }

        let used_space: f32 = results.iter().sum();
        let remaining_for_remainders = (total_width - used_space - spacing * (sizes.len() - 1) as f32).max(0.0);
        if total_remainders > 0.0 {
            let per_remainder = remaining_for_remainders / total_remainders;
            for (i, size) in sizes.iter().enumerate() {
                if let Size::Remainder { weight, range } = size {
                    let clamped = (per_remainder * weight).clamp(range.min, range.max);
                    results[i] = clamped;
                }
            }
        }

        results
    }

    
    #[inline]
    fn columns_sized<R>(
        &mut self,
        sizes: &[Size],
        add_contents: impl FnOnce(&mut [Self]) -> R,
    ) -> R {
        self.columns_sized_dyn(sizes, Box::new(add_contents))
    }

    fn columns_sized_dyn<'c, R>(
        &mut self,
        sizes: &[Size],
        add_contents: Box<dyn FnOnce(&mut [Self]) -> R + 'c>,
    ) -> R {
        let spacing = self.spacing().item_spacing.x;
        let actual_sizes = self.calc_sizes(sizes);
        let top_left = self.cursor().min;
        let mut current_left = 0.0;

        let mut columns: Vec<Self> = (0..actual_sizes.len())
            .map(|col_idx| {
                let pos = top_left + egui::vec2(current_left, 0.0);
                let cell_size = actual_sizes[col_idx];
                let child_rect = egui::Rect::from_min_max(
                    pos,
                    egui::pos2(pos.x + cell_size, self.max_rect().right_bottom().y),
                );
                current_left += cell_size + spacing;

                let mut column_ui = self.new_child(
                    egui::UiBuilder::new()
                        .max_rect(child_rect)
                        .layout(egui::Layout::top_down_justified(egui::Align::Center)),
                );
                column_ui.set_width(cell_size);
                column_ui
            })
            .collect();

        let result = add_contents(&mut columns[..]);

        let mut max_height = 0.0;
        for column in &columns {
            max_height = column.min_size().y.max(max_height);
        }

        // Make sure we fit everything next frame:
        let total_required_width = current_left - spacing; // remove last spacing

        let size = egui::vec2(self.available_width().max(total_required_width), max_height);
        self.advance_cursor_after_rect(egui::Rect::from_min_size(top_left, size));
        result
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
