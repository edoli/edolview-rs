use color_eyre::eyre::{eyre, Result};
use eframe::egui::Vec2;
use egui_glow::glow; // Re-exported glow
use glow::HasContext;

use crate::ui::gl::{ShaderBuilder};

pub struct ImageProgram {
    pub program: glow::Program,
    pub vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,

    u_texture: glow::UniformLocation,
    u_scale: glow::UniformLocation,
    u_position: glow::UniformLocation,
    u_pixel_scale: glow::UniformLocation,

    u_exposure: glow::UniformLocation,
    u_offset: glow::UniformLocation,
    u_gamma: glow::UniformLocation,
    u_min_v: glow::UniformLocation,
    u_max_v: glow::UniformLocation,
    u_r_scale: glow::UniformLocation,
    u_g_scale: glow::UniformLocation,
    u_b_scale: glow::UniformLocation,
    u_is_reciprocal: glow::UniformLocation,
}

pub struct ShaderParams {
    pub offset: f32,
    pub exposure: f32,
    pub gamma: f32,
    pub min_v: f32,
    pub max_v: f32,
    pub r_scale: f32,
    pub g_scale: f32,
    pub b_scale: f32,
    pub is_reciprocal: bool,
}

trait GlExt {
    unsafe fn check_and_get_uniform_location(&self, program: glow::Program, name: &str) -> glow::UniformLocation;
}

impl GlExt for glow::Context {
    unsafe fn check_and_get_uniform_location(&self, program: glow::Program, name: &str) -> glow::UniformLocation {
        self.get_uniform_location(program, name)
            .expect(&format!("Failed to get uniform location for {}", name))
    }
}

impl ImageProgram {
    pub fn new(gl: &glow::Context) -> Result<Self> {
        unsafe {
            let vert_src = r#"#version 330 core
                layout (location = 0) in vec2 a_pos;
                layout (location = 1) in vec2 a_tex_coord;

                out vec2 v_tex_coord;

                uniform float u_scale;
                uniform vec2  u_position;
                uniform vec2  u_pixel_scale;

                void main(){
                    v_tex_coord = a_tex_coord;
                    vec2 pos = a_pos * u_pixel_scale * 2.0 * u_scale;
                    pos.x = pos.x + u_position.x - 1.0;
                    pos.y = -(pos.y + u_position.y - 1.0);
                    gl_Position = vec4(pos, 0.0, 1.0);
                }
            "#;
            let program = gl.create_program().map_err(|e| eyre!(e))?;
            let vs = gl.create_shader(glow::VERTEX_SHADER).unwrap();
            gl.shader_source(vs, vert_src);
            gl.compile_shader(vs);
            if !gl.get_shader_compile_status(vs) {
                return Err(eyre!(gl.get_shader_info_log(vs)));
            }
            let fs = gl.create_shader(glow::FRAGMENT_SHADER).unwrap();
            gl.shader_source(fs, ShaderBuilder::new().build("color", false).as_str());
            gl.compile_shader(fs);
            if !gl.get_shader_compile_status(fs) {
                return Err(eyre!(gl.get_shader_info_log(fs)));
            }
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                return Err(eyre!(gl.get_program_info_log(program)));
            }
            gl.detach_shader(program, vs);
            gl.detach_shader(program, fs);
            gl.delete_shader(vs);
            gl.delete_shader(fs);

            #[rustfmt::skip]
            let vertices: [f32; 16] = [
                // pos      // uv
                0.0, 0.0, 0.0, 0.0,
                1.0, 0.0, 1.0, 0.0,
                1.0, 1.0, 1.0, 1.0,
                0.0, 1.0, 0.0, 1.0,
            ];
            let indices: [u32; 6] = [0, 1, 2, 2, 3, 0];

            let vao = gl.create_vertex_array().unwrap();
            let vbo = gl.create_buffer().unwrap();
            let ebo = gl.create_buffer().unwrap();
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck::cast_slice(&vertices), glow::STATIC_DRAW);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytemuck::cast_slice(&indices), glow::STATIC_DRAW);
            let stride = (4 * std::mem::size_of::<f32>()) as i32;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, 2 * 4);
            gl.bind_vertex_array(None);

            let u_texture = gl.check_and_get_uniform_location(program, "u_texture");
            let u_scale = gl.check_and_get_uniform_location(program, "u_scale");
            let u_position = gl.check_and_get_uniform_location(program, "u_position");
            let u_pixel_scale = gl.check_and_get_uniform_location(program, "u_pixel_scale");

            let u_offset = gl.check_and_get_uniform_location(program, "u_offset");
            let u_exposure = gl.check_and_get_uniform_location(program, "u_exposure");
            let u_gamma = gl.check_and_get_uniform_location(program, "u_gamma");
            let u_min_v = gl.check_and_get_uniform_location(program, "u_min_v");
            let u_max_v = gl.check_and_get_uniform_location(program, "u_max_v");
            let u_r_scale = gl.check_and_get_uniform_location(program, "u_r_scale");
            let u_g_scale = gl.check_and_get_uniform_location(program, "u_g_scale");
            let u_b_scale = gl.check_and_get_uniform_location(program, "u_b_scale");
            let u_is_reciprocal = gl.check_and_get_uniform_location(program, "u_is_reciprocal");

            Ok(Self {
                program,
                vao,
                vbo,
                ebo,
                u_texture,
                u_scale,
                u_position,
                u_pixel_scale,
                u_offset,
                u_exposure,
                u_gamma,
                u_min_v,
                u_max_v,
                u_r_scale,
                u_g_scale,
                u_b_scale,
                u_is_reciprocal,
            })
        }
    }

    pub unsafe fn draw(
        &self,
        gl: &glow::Context,
        tex_id: glow::NativeTexture,
        scale: f32,
        position: Vec2,
        pixel_scale: Vec2,
        shader_params: ShaderParams,
    ) {
        gl.use_program(Some(self.program));
        gl.enable(glow::BLEND);
        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(tex_id));

        gl.uniform_1_i32(Some(&self.u_texture), 0);
        gl.uniform_1_f32(Some(&self.u_scale), scale);
        gl.uniform_2_f32(Some(&self.u_position), position.x, position.y);
        gl.uniform_2_f32(Some(&self.u_pixel_scale), pixel_scale.x, pixel_scale.y);

        gl.uniform_1_f32(Some(&self.u_exposure), shader_params.exposure);
        gl.uniform_1_f32(Some(&self.u_offset), shader_params.offset);
        gl.uniform_1_f32(Some(&self.u_gamma), shader_params.gamma);
        gl.uniform_1_f32(Some(&self.u_min_v), shader_params.min_v);
        gl.uniform_1_f32(Some(&self.u_max_v), shader_params.max_v);
        gl.uniform_1_f32(Some(&self.u_r_scale), shader_params.r_scale);
        gl.uniform_1_f32(Some(&self.u_g_scale), shader_params.g_scale);
        gl.uniform_1_f32(Some(&self.u_b_scale), shader_params.b_scale);
        gl.uniform_1_i32(Some(&self.u_is_reciprocal), shader_params.is_reciprocal as i32);

        gl.bind_vertex_array(Some(self.vao));
        gl.draw_elements(glow::TRIANGLES, 6, glow::UNSIGNED_INT, 0);
        gl.bind_vertex_array(None);
        gl.use_program(None);
    }

    pub unsafe fn destroy(&self, gl: &glow::Context) {
        // Ignore any errors; we're in Drop.
        gl.delete_program(self.program);
        gl.delete_vertex_array(self.vao);
        gl.delete_buffer(self.vbo);
        gl.delete_buffer(self.ebo);
    }
}
