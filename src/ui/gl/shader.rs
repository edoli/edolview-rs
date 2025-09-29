use color_eyre::eyre::{eyre, Result};
use eframe::egui::Vec2;
use egui_glow::glow; // Re-exported glow
use glow::HasContext;

use crate::{
    model::MinMax,
    switch,
    ui::gl::{gl_ext::GlExt, ShaderBuilder},
};

pub struct ImageProgram {
    pub program: glow::Program,
    pub vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,

    shader_builder: ShaderBuilder,
    last_color_map_name: String,
    last_is_mono: bool,

    u_viewport_size: glow::UniformLocation,
    u_image_size: glow::UniformLocation,

    u_texture: glow::UniformLocation,
    u_channel_index: glow::UniformLocation,
    u_scale: glow::UniformLocation,
    u_position: glow::UniformLocation,

    u_use_alpha: glow::UniformLocation,

    u_exposure: glow::UniformLocation,
    u_offset: glow::UniformLocation,
    u_gamma: glow::UniformLocation,

    u_min_v: glow::UniformLocation,
    u_max_v: glow::UniformLocation,
    u_scale_mode: glow::UniformLocation,

    u_use_per_channel: glow::UniformLocation,

    u_min_v_chs: [glow::UniformLocation; 4],
    u_max_v_chs: [glow::UniformLocation; 4],
    u_scale_mode_chs: [glow::UniformLocation; 4],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScaleMode {
    Linear = 0,
    Inverse = 1,
    Log = 2,
}

impl Default for ScaleMode {
    fn default() -> Self {
        ScaleMode::Linear
    }
}

#[derive(Clone)]
pub struct ShaderParams {
    pub use_alpha: bool,
    pub offset: f32,
    pub exposure: f32,
    pub gamma: f32,
    pub min_v: f32,
    pub max_v: f32,
    pub auto_minmax: bool,
    pub scale_mode: ScaleMode,
    pub use_per_channel: bool,
    pub min_v_channels: [f32; 4],
    pub max_v_channels: [f32; 4],
    pub auto_minmax_channels: [bool; 4],
    pub scale_mode_channels: [ScaleMode; 4],
}

impl Default for ShaderParams {
    fn default() -> Self {
        Self {
            use_alpha: true,
            offset: 0.0,
            exposure: 0.0,
            gamma: 1.0,
            min_v: 0.0,
            max_v: 1.0,
            auto_minmax: false,
            scale_mode: ScaleMode::Linear,
            use_per_channel: false,
            min_v_channels: [0.0; 4],
            max_v_channels: [1.0; 4],
            auto_minmax_channels: [false; 4],
            scale_mode_channels: [ScaleMode::Linear; 4],
        }
    }
}

const VERT_SRC: &str = r#"#version 330 core
    layout (location = 0) in vec2 a_pos;
    layout (location = 1) in vec2 a_tex_coord;

    out vec2 v_tex_coord;

    uniform vec2 u_viewport_size;
    uniform vec2 u_image_size;
    uniform float u_scale;
    uniform vec2 u_position;

    void main(){
        v_tex_coord = a_tex_coord;
        vec2 pos = (a_pos * u_image_size * 2.0 * u_scale) / u_viewport_size;
        pos.x = pos.x + u_position.x / u_viewport_size.x * 2.0 - 1.0;
        pos.y = -(pos.y + u_position.y / u_viewport_size.y * 2.0 - 1.0);
        gl_Position = vec4(pos, 0.0, 1.0);
    }
"#;

unsafe fn compile_colormap_shader(
    gl: &glow::Context,
    shader_builder: &ShaderBuilder,
    colormap_name: &str,
    is_mono: bool,
) -> Result<glow::Program> {
    let program = gl.create_program().map_err(|e| eyre!(e))?;
    let vs = gl.create_shader(glow::VERTEX_SHADER).unwrap();
    gl.shader_source(vs, VERT_SRC);
    gl.compile_shader(vs);
    if !gl.get_shader_compile_status(vs) {
        return Err(eyre!(gl.get_shader_info_log(vs)));
    }

    let fs = gl.create_shader(glow::FRAGMENT_SHADER).unwrap();
    gl.shader_source(fs, shader_builder.build(colormap_name, is_mono).as_str());
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

    Ok(program)
}

impl ImageProgram {
    pub fn new(gl: &glow::Context) -> Result<Self> {
        unsafe {
            let last_color_map_name = "rgb".to_string();
            let last_is_mono = false;
            let shader_builder = ShaderBuilder::new();
            let program = compile_colormap_shader(gl, &shader_builder, last_color_map_name.as_str(), last_is_mono)
                .map_err(|e| {
                    eprintln!("Failed to compile colormap shader '{}': {e}", last_color_map_name);
                    e
                })?;

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

            let u_viewport_size = gl.check_and_get_uniform_location(program, "u_viewport_size");
            let u_image_size = gl.check_and_get_uniform_location(program, "u_image_size");

            let u_texture = gl.check_and_get_uniform_location(program, "u_texture");
            let u_channel_index = gl.check_and_get_uniform_location(program, "u_channel_index");
            let u_scale = gl.check_and_get_uniform_location(program, "u_scale");
            let u_position = gl.check_and_get_uniform_location(program, "u_position");

            let u_use_alpha = gl.check_and_get_uniform_location(program, "u_use_alpha");
            let u_offset = gl.check_and_get_uniform_location(program, "u_offset");
            let u_exposure = gl.check_and_get_uniform_location(program, "u_exposure");
            let u_gamma = gl.check_and_get_uniform_location(program, "u_gamma");
            let u_min_v = gl.check_and_get_uniform_location(program, "u_min_v");
            let u_max_v = gl.check_and_get_uniform_location(program, "u_max_v");
            let u_scale_mode = gl.check_and_get_uniform_location(program, "u_scale_mode");

            let u_use_per_channel = gl.check_and_get_uniform_location(program, "u_use_per_channel");

            let u_min_v_chs =
                std::array::from_fn(|i| gl.check_and_get_uniform_location(program, &format!("u_min_v{i}")));
            let u_max_v_chs =
                std::array::from_fn(|i| gl.check_and_get_uniform_location(program, &format!("u_max_v{i}")));
            let u_scale_mode_chs =
                std::array::from_fn(|i| gl.check_and_get_uniform_location(program, &format!("u_scale_mode{i}")));

            Ok(Self {
                program,
                vao,
                vbo,
                ebo,
                shader_builder,
                last_color_map_name,
                last_is_mono,
                u_viewport_size,
                u_image_size,
                u_texture,
                u_scale,
                u_position,
                u_channel_index,
                u_use_alpha,
                u_offset,
                u_exposure,
                u_gamma,
                u_min_v,
                u_max_v,
                u_scale_mode,
                u_use_per_channel,
                u_min_v_chs,
                u_max_v_chs,
                u_scale_mode_chs,
            })
        }
    }

    pub unsafe fn update_uniforms(&mut self, gl: &glow::Context) {
        let program = self.program;

        self.u_viewport_size = gl.check_and_get_uniform_location(program, "u_viewport_size");
        self.u_image_size = gl.check_and_get_uniform_location(program, "u_image_size");

        self.u_texture = gl.check_and_get_uniform_location(program, "u_texture");
        self.u_channel_index = gl.check_and_get_uniform_location(program, "u_channel_index");
        self.u_scale = gl.check_and_get_uniform_location(program, "u_scale");
        self.u_position = gl.check_and_get_uniform_location(program, "u_position");

        self.u_use_alpha = gl.check_and_get_uniform_location(program, "u_use_alpha");
        self.u_offset = gl.check_and_get_uniform_location(program, "u_offset");
        self.u_exposure = gl.check_and_get_uniform_location(program, "u_exposure");
        self.u_gamma = gl.check_and_get_uniform_location(program, "u_gamma");
        self.u_min_v = gl.check_and_get_uniform_location(program, "u_min_v");
        self.u_max_v = gl.check_and_get_uniform_location(program, "u_max_v");
        self.u_scale_mode = gl.check_and_get_uniform_location(program, "u_scale_mode");

        self.u_use_per_channel = gl.check_and_get_uniform_location(program, "u_use_per_channel");

        self.u_min_v_chs = std::array::from_fn(|i| gl.check_and_get_uniform_location(program, &format!("u_min_v{i}")));
        self.u_max_v_chs = std::array::from_fn(|i| gl.check_and_get_uniform_location(program, &format!("u_max_v{i}")));
        self.u_scale_mode_chs =
            std::array::from_fn(|i| gl.check_and_get_uniform_location(program, &format!("u_scale_mode{i}")));
    }

    pub unsafe fn draw(
        &mut self,
        gl: &glow::Context,
        tex_id: glow::NativeTexture,
        colormap_name: &str,
        viewport_size: Vec2,
        image_size: Vec2,
        channel_index: i32,
        min_max: &MinMax,
        is_mono: bool,
        scale: f32,
        position: Vec2,
        shader_params: &ShaderParams,
    ) {
        if self.last_color_map_name != colormap_name || self.last_is_mono != is_mono {
            if let Ok(new_program) = compile_colormap_shader(gl, &self.shader_builder, colormap_name, is_mono) {
                gl.delete_program(self.program);
                self.program = new_program;
                self.update_uniforms(gl);
                self.last_color_map_name = colormap_name.to_string();
                self.last_is_mono = is_mono;
            } else {
                eprintln!("Failed to compile colormap shader: {}", colormap_name);
            }
        }

        gl.use_program(Some(self.program));
        gl.enable(glow::BLEND);
        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(tex_id));

        gl.uniform_2_f32v(Some(&self.u_viewport_size), viewport_size);
        gl.uniform_2_f32v(Some(&self.u_image_size), image_size);

        gl.uniform_1_i32(Some(&self.u_texture), 0);
        gl.uniform_1_i32(Some(&self.u_channel_index), channel_index);
        gl.uniform_1_f32(Some(&self.u_scale), scale);
        gl.uniform_2_f32v(Some(&self.u_position), position);

        gl.uniform_1_i32(Some(&self.u_use_alpha), if shader_params.use_alpha { 1 } else { 0 });

        gl.uniform_1_f32(Some(&self.u_exposure), shader_params.exposure);
        gl.uniform_1_f32(Some(&self.u_offset), shader_params.offset);
        gl.uniform_1_f32(Some(&self.u_gamma), shader_params.gamma);

        gl.uniform_1_i32(Some(&self.u_use_per_channel), if shader_params.use_per_channel { 1 } else { 0 });

        if !shader_params.use_per_channel {
            let auto_minmax = shader_params.auto_minmax;
            gl.uniform_1_f32(
                Some(&self.u_min_v),
                switch!(auto_minmax => min_max.total_min(), shader_params.min_v),
            );
            gl.uniform_1_f32(
                Some(&self.u_max_v),
                switch!(auto_minmax => min_max.total_max(), shader_params.max_v),
            );
            gl.uniform_1_i32(Some(&self.u_scale_mode), shader_params.scale_mode as i32);
        } else {
            let min_v_chs = &shader_params.min_v_channels;
            let max_v_chs = &shader_params.max_v_channels;
            let scale_mode_chs = &shader_params.scale_mode_channels;

            for i in 0..4 {
                let auto_minmax = shader_params.auto_minmax_channels[i];
                gl.uniform_1_f32(Some(&self.u_min_v_chs[i]), switch!(auto_minmax => min_max.min(i), min_v_chs[i]));
                gl.uniform_1_f32(Some(&self.u_max_v_chs[i]), switch!(auto_minmax => min_max.max(i), max_v_chs[i]));
                gl.uniform_1_i32(Some(&self.u_scale_mode_chs[i]), scale_mode_chs[i] as i32);
            }
        }

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
