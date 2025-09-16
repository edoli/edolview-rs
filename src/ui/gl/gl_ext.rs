use eframe::{
    egui::{Color32, Vec2},
    glow::{self, HasContext, UniformLocation},
};

pub trait GlExt {
    unsafe fn check_and_get_uniform_location(&self, program: glow::Program, name: &str) -> glow::UniformLocation;

    unsafe fn uniform_2_f32v(&self, location: Option<&UniformLocation>, vec: Vec2);
    unsafe fn uniform_4_f32c(&self, location: Option<&UniformLocation>, color: Color32);
}

impl GlExt for glow::Context {
    unsafe fn check_and_get_uniform_location(&self, program: glow::Program, name: &str) -> glow::UniformLocation {
        self.get_uniform_location(program, name)
            .expect(&format!("Failed to get uniform location for {}", name))
    }

    unsafe fn uniform_2_f32v(&self, location: Option<&UniformLocation>, vec: Vec2) {
        self.uniform_2_f32(location, vec.x, vec.y);
    }

    unsafe fn uniform_4_f32c(&self, location: Option<&UniformLocation>, color: Color32) {
        self.uniform_4_f32(
            location,
            color.r() as f32 / 255.0,
            color.g() as f32 / 255.0,
            color.b() as f32 / 255.0,
            color.a() as f32 / 255.0,
        );
    }
}
