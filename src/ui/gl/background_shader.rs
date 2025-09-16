use color_eyre::eyre::{eyre, Result};
use eframe::{
    egui::{Color32, Vec2},
    glow,
};
use glow::HasContext;

use crate::ui::gl::gl_ext::GlExt;

pub struct BackgroundProgram {
    pub program: glow::Program,
    pub vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,

    u_viewport_size: glow::UniformLocation,
    u_position: glow::UniformLocation,
    u_grid_size: glow::UniformLocation,
    u_grid_color_a: glow::UniformLocation,
    u_grid_color_b: glow::UniformLocation,
}

impl BackgroundProgram {
    pub fn new(gl: &glow::Context) -> Result<Self> {
        unsafe {
            // Simple full-screen quad (two triangles) in NDC
            let vert_src = r#"
                #version 330 core
                layout (location = 0) in vec2 a_pos;

                out vec2 v_uv;

                void main(){
                    v_uv = (a_pos + 1.0) * 0.5; // convert from [-1,1] to [0,1]
                    v_uv.y = 1.0 - v_uv.y; // flip Y for texture coords
                    gl_Position = vec4(a_pos, 0.0, 1.0);
                }
            "#;

            // Checker pattern: alternating colors based on floor of uv * grid_size
            let frag_src = r#"
                #version 330 core
                in vec2 v_uv;

                out vec4 FragColor;

                uniform vec2 u_viewport_size;
                uniform vec2 u_position;
                uniform float u_grid_size;
                uniform vec4 u_grid_color_a;
                uniform vec4 u_grid_color_b;

                void main(){
                    vec2 g = floor((v_uv * u_viewport_size - u_position) / u_grid_size);
                    float parity = mod(g.x + g.y, 2.0);
                    FragColor = mix(u_grid_color_a, u_grid_color_b, parity);
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
            gl.shader_source(fs, frag_src);
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

            // Fullscreen quad (two triangles) covering NDC
            #[rustfmt::skip]
            let vertices: [f32; 8] = [
                -1.0, -1.0,
                 1.0, -1.0,
                 1.0,  1.0,
                -1.0,  1.0,
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
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 2 * 4, 0);
            gl.bind_vertex_array(None);

            let u_viewport_size = gl.check_and_get_uniform_location(program, "u_viewport_size");
            let u_position = gl.check_and_get_uniform_location(program, "u_position");
            let u_grid_size = gl.check_and_get_uniform_location(program, "u_grid_size");
            let u_grid_color_a = gl.check_and_get_uniform_location(program, "u_grid_color_a");
            let u_grid_color_b = gl.check_and_get_uniform_location(program, "u_grid_color_b");

            Ok(Self {
                program,
                vao,
                vbo,
                ebo,
                u_viewport_size,
                u_position,
                u_grid_size,
                u_grid_color_a,
                u_grid_color_b,
            })
        }
    }

    pub unsafe fn draw(
        &self,
        gl: &glow::Context,
        viewport_size: Vec2,
        position: Vec2,
        grid_size: f32,
        grid_color_a: Color32,
        grid_color_b: Color32,
    ) {
        gl.use_program(Some(self.program));
        gl.disable(glow::BLEND);
        gl.uniform_2_f32v(Some(&self.u_viewport_size), viewport_size);
        gl.uniform_2_f32v(Some(&self.u_position), position);
        gl.uniform_1_f32(Some(&self.u_grid_size), grid_size);
        gl.uniform_4_f32c(Some(&self.u_grid_color_a), grid_color_a);
        gl.uniform_4_f32c(Some(&self.u_grid_color_b), grid_color_b);
        gl.bind_vertex_array(Some(self.vao));
        gl.draw_elements(glow::TRIANGLES, 6, glow::UNSIGNED_INT, 0);
        gl.bind_vertex_array(None);
        gl.use_program(None);
    }

    pub unsafe fn destroy(&self, gl: &glow::Context) {
        gl.delete_program(self.program);
        gl.delete_vertex_array(self.vao);
        gl.delete_buffer(self.vbo);
        gl.delete_buffer(self.ebo);
    }
}
