use crate::util::str_ext::Join;
use eframe::egui::Color32;

fn to_color32(color: &Vec<f32>) -> Color32 {
    if color.len() == 1 {
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
    }
}

fn to_hex_string(color: &Vec<f32>) -> String {
    if color.len() == 1 {
        format!("#{:02X}", (color[0] * 255.0) as u8)
    } else if color.len() == 2 {
        format!("#{:02X}{:02X}", (color[0] * 255.0) as u8, (color[1] * 255.0) as u8)
    } else if color.len() == 3 {
        format!(
            "#{:02X}{:02X}{:02X}",
            (color[0] * 255.0) as u8,
            (color[1] * 255.0) as u8,
            (color[2] * 255.0) as u8
        )
    } else if color.len() == 4 {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            (color[0] * 255.0) as u8,
            (color[1] * 255.0) as u8,
            (color[2] * 255.0) as u8,
            (color[3] * 255.0) as u8
        )
    } else {
        String::from("#000000")
    }
}

fn to_rgba_string(color: &Vec<f32>) -> String {
    format!("({})", color.join(", "))
}

fn to_rgba_int_string(color: &Vec<f32>, alpha: f64) -> String {
    format!(
        "({})",
        color
            .iter()
            .map(|c| (((*c as f64) * alpha) as i32).to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub trait ColorDisplay {
    fn to_color32(&self) -> Color32;
    fn to_hex_string(&self) -> String;
    fn to_rgba_string(&self) -> String;
    fn to_rgba_int_string(&self, alpha: f64) -> String;
}

impl ColorDisplay for Vec<f32> {
    fn to_color32(&self) -> Color32 {
        to_color32(self)
    }

    fn to_hex_string(&self) -> String {
        to_hex_string(self)
    }

    fn to_rgba_string(&self) -> String {
        to_rgba_string(self)
    }

    fn to_rgba_int_string(&self, alpha: f64) -> String {
        to_rgba_int_string(self, alpha)
    }
}
